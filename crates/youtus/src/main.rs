//! Point de composition : la seule crate qui connaît à la fois l'UI, le
//! réseau, la base et le lecteur. Tout ce qui bloque vit dans `worker` ou dans
//! les threads de mpv ; ici on ne fait que traduire des événements en
//! propriétés Slint.
//!
//! Vidéo : libmpv en API render, jamais `--wid` (impossible sous Wayland).
//! Le contexte de rendu est créé dans le notifier Slint, la texture est
//! affichée via `slint::Image`, et les frames sont consommées par un timer UI.

mod config;
mod worker;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Timer};
use youtus_core::{format_duration, sanitize_time, FeedKind};
use youtus_play::{GlVideo, MpvPlayer, VIDEO_HEIGHT, VIDEO_WIDTH};
use youtus_ui::App;

use config::Config;
use worker::Event;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .iter()
        .any(|argument| argument == "-h" || argument == "--help")
    {
        print!("{}", config::USAGE);
        return Ok(());
    }
    if args
        .iter()
        .any(|argument| argument == "-V" || argument == "--version")
    {
        println!("youtus {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let config = match Config::parse(args) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    let app = Rc::new(App::new()?);
    app.set_page_title("Accueil");
    app.set_status(&config.summary());

    let (commands, events) = worker::spawn(config);

    let player = match MpvPlayer::new() {
        Ok(player) => Some(Rc::new(player)),
        Err(error) => {
            eprintln!("youtus: libmpv indisponible : {error:#}");
            app.set_error(Some(
                "libmpv indisponible : la lecture vidéo est désactivée.",
            ));
            None
        }
    };

    let runtime = Rc::new(RefCell::new(Runtime {
        app: Rc::clone(&app),
        player: player.as_ref().map(Rc::clone),
        commands: commands.clone(),
        watch_open: false,
        current: None,
        last_save: Instant::now(),
        pending_seek: None,
        pending_audio: None,
        generation: 0,
        close_at: None,
    }));

    // Slot texture → image : `slint::Image` n'est pas `Send`, il ne doit jamais
    // traverser un canal. Le notifier et le timer tournent sur le thread UI.
    let video_slot: Rc<RefCell<Option<GlVideo>>> = Rc::new(RefCell::new(None));
    let image_slot: Rc<RefCell<Option<slint::Image>>> = Rc::new(RefCell::new(None));
    let frame_pending = Arc::new(AtomicBool::new(false));
    let frames = Arc::new(AtomicU64::new(0));

    if let Some(player) = &player {
        let player = Rc::clone(player);
        let video_setup = Rc::clone(&video_slot);
        let video_render = Rc::clone(&video_slot);
        let images = Rc::clone(&image_slot);
        let pending = Arc::clone(&frame_pending);
        let frames = Arc::clone(&frames);

        app.window()
            .window()
            .set_rendering_notifier(move |state, api| match state {
                slint::RenderingState::RenderingSetup => {
                    let loader = match &api {
                        slint::GraphicsAPI::NativeOpenGL { get_proc_address } => *get_proc_address,
                        _ => {
                            eprintln!(
                                "youtus: le renderer Slint n'utilise pas OpenGL — vidéo désactivée"
                            );
                            return;
                        }
                    };

                    match GlVideo::setup(loader, player.mpv()) {
                        Ok(mut video) => {
                            let pending = Arc::clone(&pending);
                            // Appelé depuis un thread mpv : uniquement un drapeau.
                            video.set_update_callback(move || {
                                pending.store(true, Ordering::Release);
                            });

                            // SAFETY: la texture appartient à `video`, conservé
                            // dans `video_setup` aussi longtemps que la fenêtre.
                            let image = unsafe {
                                slint::BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                                    video.texture,
                                    (VIDEO_WIDTH, VIDEO_HEIGHT).into(),
                                )
                                .build()
                            };
                            *images.borrow_mut() = Some(image);
                            *video_setup.borrow_mut() = Some(video);
                            eprintln!(
                                "youtus: vidéo prête ({VIDEO_WIDTH}x{VIDEO_HEIGHT}, API render)"
                            );
                        }
                        Err(error) => eprintln!("youtus: vidéo indisponible : {error:#}"),
                    }
                }
                slint::RenderingState::BeforeRendering => {
                    if let Some(video) = video_render.borrow().as_ref() {
                        // À l'EOF mpv présente une frame vide : on garde la
                        // dernière image au lieu de noircir la texture.
                        if pending.swap(false, Ordering::AcqRel) && !player.eof_reached() {
                            match video.render() {
                                Ok(()) => {
                                    if frames.fetch_add(1, Ordering::Relaxed) == 0 {
                                        eprintln!("youtus: première frame vidéo rendue");
                                    }
                                }
                                Err(error) => eprintln!("youtus: rendu mpv : {error:#}"),
                            }
                        }
                    }
                }
                slint::RenderingState::RenderingTeardown => {
                    *video_render.borrow_mut() = None;
                }
                _ => {}
            })?;
    }

    let ui_timer = Timer::default();
    {
        let runtime = Rc::clone(&runtime);
        let events = events.clone();
        let images = Rc::clone(&image_slot);
        let pending = Arc::clone(&frame_pending);
        ui_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(8),
            move || {
                if let Some(image) = images.borrow_mut().take() {
                    runtime.borrow().app.set_video_frame(image);
                }
                while let Ok(event) = events.try_recv() {
                    runtime.borrow_mut().handle(event);
                }
                // Sans redraw, Slint n'appelle jamais BeforeRendering et mpv
                // n'est jamais pompé : la lecture resterait figée.
                if pending.load(Ordering::Acquire) {
                    runtime.borrow().app.window().window().request_redraw();
                }
            },
        );
    }

    let playback_timer = Timer::default();
    {
        let runtime = Rc::clone(&runtime);
        playback_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(250),
            move || runtime.borrow_mut().tick(),
        );
    }

    {
        let runtime = Rc::clone(&runtime);
        app.on_command(move |command| runtime.borrow_mut().command(command));
    }

    // Crochet de vérification : joue une vidéo dès que le worker est prêt.
    if let Ok(id) = std::env::var("YOUTUS_AUTOPLAY") {
        if !id.is_empty() {
            // Session de vérification : on ne veut pas de son sur le bureau.
            if let Some(player) = &player {
                let _ = player.set_volume(0.0);
            }
            let _ = commands.send(worker::Command::Play {
                title: format!("Vidéo {id}"),
                id,
            });
        }
    }

    app.run()?;

    runtime.borrow_mut().close_watch(true);
    Ok(())
}

fn feed_kind(page: &str) -> Option<FeedKind> {
    match page {
        "home" => Some(FeedKind::Home),
        "subscriptions" => Some(FeedKind::Subscriptions),
        "history" => Some(FeedKind::History),
        "trending" => Some(FeedKind::Trending),
        _ => None,
    }
}

fn clock(seconds: f64) -> String {
    format_duration(Duration::from_secs_f64(seconds.max(0.0)))
}

struct Runtime {
    app: Rc<App>,
    player: Option<Rc<MpvPlayer>>,
    commands: crossbeam_channel::Sender<worker::Command>,
    watch_open: bool,
    current: Option<String>,
    last_save: Instant,
    /// Seek et piste audio à appliquer dès que mpv connaît la durée.
    pending_seek: Option<f64>,
    pending_audio: Option<String>,
    generation: u64,
    /// Crochet de test : ferme l'écran Watch à cet instant, exactement comme
    /// le bouton Fermer (`YOUTUS_CLOSE_AFTER`).
    close_at: Option<Instant>,
}

impl Runtime {
    fn handle(&mut self, event: Event) {
        match event {
            Event::Loading { page } => {
                self.app.set_page_title(&page);
                self.app.set_loading(true);
                self.app.set_error(None);
            }
            Event::Feed { generation, videos } => {
                self.generation = generation;
                self.app.set_videos(&videos);
            }
            Event::Thumbnail {
                generation,
                index,
                width,
                height,
                rgba,
            } => {
                if generation != self.generation {
                    return;
                }
                if let Some(image) = youtus_ui::image_from_rgba(&rgba, width, height) {
                    self.app.set_thumbnail(index, image);
                }
            }
            Event::Error(message) => {
                eprintln!("youtus: {message}");
                self.app.set_loading(false);
                self.app.set_error(Some(&message));
            }
            Event::Notice(message) => {
                eprintln!("youtus: {message}");
                self.app.set_status(&message);
            }
            Event::SignedOut => {
                self.app.set_loading(false);
                self.app.set_error(None);
                self.app.set_signed_out(true);
                self.close_watch(true);
            }
            Event::SignedIn(label) => {
                self.app.set_signed_out(false);
                self.app.set_user_code("");
                self.app.set_status(&label);
            }
            Event::DeviceCode {
                user_code,
                verification_url,
            } => {
                self.app.set_signed_out(true);
                self.app.set_user_code(&user_code);
                self.app.set_verification_url(&verification_url);
                self.app.set_status("En attente de l'autorisation…");
            }
            Event::Playback {
                video_id,
                title,
                url,
                audio_url,
                headers,
                start_at,
            } => self.start_playback(video_id, title, url, audio_url, headers, start_at),
        }
    }

    fn start_playback(
        &mut self,
        video_id: String,
        title: String,
        url: String,
        audio_url: Option<String>,
        headers: Vec<(String, String)>,
        start_at: Option<f64>,
    ) {
        let Some(player) = self.player.clone() else {
            self.app.set_error(Some(
                "Lecteur vidéo indisponible : libmpv ou rendu OpenGL manquant.",
            ));
            return;
        };

        match player.load(&url, &headers) {
            Ok(()) => {
                eprintln!(
                    "youtus: lecture de {video_id} (reprise à {:.1} s)",
                    start_at.unwrap_or(0.0)
                );
                self.pending_seek = start_at.filter(|seconds| *seconds > 0.0);
                self.pending_audio = audio_url;
                self.app.set_error(None);
                self.app.set_watch_title(&title);
                self.app.set_watch_open(true);
                self.app.set_playing(true);
                self.app.set_progress(0.0);
                self.app.set_time_label("0:00 / 0:00");
                self.watch_open = true;
                self.current = Some(video_id);
                self.last_save = Instant::now();
                self.close_at = std::env::var("YOUTUS_CLOSE_AFTER")
                    .ok()
                    .and_then(|seconds| seconds.parse::<u64>().ok())
                    .map(|seconds| Instant::now() + Duration::from_secs(seconds));
            }
            Err(error) => {
                eprintln!("youtus: chargement mpv : {error:#}");
                self.app
                    .set_error(Some(&format!("Lecture impossible : {error}")));
            }
        }
    }

    fn tick(&mut self) {
        let Some(player) = self.player.clone() else {
            return;
        };
        if !self.watch_open {
            return;
        }

        let (position, duration) = sanitize_time(player.position(), player.duration());
        self.app.set_playing(!player.paused() && !player.eof_reached());
        self.app.set_progress(if duration > 0.0 {
            (position / duration) as f32
        } else {
            0.0
        });
        self.app
            .set_time_label(&format!("{} / {}", clock(position), clock(duration)));

        // La durée n'est connue qu'une fois le fichier ouvert : c'est le bon
        // moment pour appliquer la reprise et la piste audio DASH.
        if duration > 0.0 {
            if let Some(target) = self.pending_seek.take() {
                let _ = player.seek(target);
                eprintln!("youtus: reprise à {target:.1} s");
            }
            if let Some(audio) = self.pending_audio.take() {
                match player.add_audio(&audio) {
                    Ok(()) => eprintln!("youtus: piste audio DASH ajoutée"),
                    Err(error) => eprintln!("youtus: piste audio : {error:#}"),
                }
            }
        }

        if self.last_save.elapsed() >= Duration::from_secs(5) && position > 0.0 {
            self.save(position, duration);
        }

        if self.close_at.is_some_and(|at| Instant::now() >= at) {
            self.close_at = None;
            eprintln!("youtus: fermeture de l'écran Watch (YOUTUS_CLOSE_AFTER)");
            self.close_watch(true);
        }
    }

    fn save(&mut self, position: f64, duration: f64) {
        let Some(video_id) = self.current.clone() else {
            return;
        };
        let _ = self.commands.send(worker::Command::Progress {
            video_id,
            position_ms: (position.max(0.0) * 1000.0).round() as u64,
            duration_ms: (duration.max(0.0) * 1000.0).round() as u64,
        });
        self.last_save = Instant::now();
    }

    fn close_watch(&mut self, stop: bool) {
        if !self.watch_open {
            return;
        }
        if let Some(player) = self.player.clone() {
            let (position, duration) = sanitize_time(player.position(), player.duration());
            self.save(position, duration);
            if let Some(video_id) = self.current.clone() {
                let _ = self.commands.send(worker::Command::Closed {
                    video_id,
                    position_ms: (position.max(0.0) * 1000.0).round() as u64,
                    duration_ms: (duration.max(0.0) * 1000.0).round() as u64,
                });
            }
            let _ = player.set_paused(true);
            if stop {
                let _ = player.stop();
            }
        }

        self.watch_open = false;
        self.current = None;
        self.pending_seek = None;
        self.pending_audio = None;
        self.app.set_watch_open(false);
        self.app.set_playing(false);
        self.app.set_progress(0.0);
        self.app.set_time_label("0:00 / 0:00");
    }

    fn command(&mut self, command: youtus_ui::Command) {
        match command {
            youtus_ui::Command::Navigate(page) => {
                self.close_watch(true);
                if let Some(kind) = feed_kind(&page) {
                    let _ = self.commands.send(worker::Command::Load(kind));
                }
            }
            youtus_ui::Command::Retry => {
                let _ = self.commands.send(worker::Command::Retry);
            }
            youtus_ui::Command::SignIn => {
                let _ = self.commands.send(worker::Command::SignIn);
            }
            youtus_ui::Command::Play { id, title } => {
                let _ = self.commands.send(worker::Command::Play { id, title });
            }
            youtus_ui::Command::CloseWatch => self.close_watch(true),
            youtus_ui::Command::TogglePlay => {
                if let Some(player) = self.player.clone() {
                    let paused = player.paused();
                    let _ = player.set_paused(!paused);
                    self.app.set_playing(paused);
                }
            }
            youtus_ui::Command::Seek(fraction) => {
                if let Some(player) = self.player.clone() {
                    let (_, duration) = sanitize_time(player.position(), player.duration());
                    if duration > 0.0 {
                        let target = (fraction as f64 * duration).clamp(0.0, duration);
                        let _ = player.seek(target);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_pages_de_la_barre_laterale_sont_connues() {
        assert_eq!(feed_kind("home"), Some(FeedKind::Home));
        assert_eq!(feed_kind("subscriptions"), Some(FeedKind::Subscriptions));
        assert_eq!(feed_kind("history"), Some(FeedKind::History));
        assert_eq!(feed_kind("trending"), Some(FeedKind::Trending));
        assert_eq!(feed_kind("playlists"), None);
    }

    #[test]
    fn assainit_les_temps_de_mpv() {
        assert_eq!(sanitize_time(f64::NAN, f64::NAN), (0.0, 0.0));
        assert_eq!(sanitize_time(12.0, 100.0), (12.0, 100.0));
        assert_eq!(sanitize_time(120.0, 100.0), (100.0, 100.0));
    }

    #[test]
    fn affiche_les_temps_en_minutes() {
        assert_eq!(clock(f64::NAN), "0:00");
        assert_eq!(clock(213.0), "3:33");
        assert_eq!(clock(3723.0), "1:02:03");
    }
}
