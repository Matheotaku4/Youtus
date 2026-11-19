mod gl;
mod player;

slint::include_modules!();

use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Mise à jour d'UI produite par le notifier de rendu (thread UI) et appliquée
/// par le timer, hors de la phase de rendu. `slint::Image` et `slint::Weak`
/// n'étant pas `Send`, elles ne doivent jamais traverser un canal inter-thread.
enum UiUpdate {
    Frame(slint::Image, String),
    Status(String),
}

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/m0.mp4".to_string());

    let app = AppWindow::new()?;
    let player = Rc::new(player::MpvPlayer::new()?);
    let video: Rc<RefCell<Option<gl::GlVideo>>> = Rc::new(RefCell::new(None));
    let frame_pending = Arc::new(AtomicBool::new(true));
    let frames = Arc::new(AtomicU64::new(0));
    let render_us = Arc::new(AtomicU64::new(0));
    let pending_ui: Rc<RefCell<Option<UiUpdate>>> = Rc::new(RefCell::new(None));

    {
        let player = player.clone();
        let video = video.clone();
        let frame_pending = frame_pending.clone();
        let frames = frames.clone();
        let render_us = render_us.clone();
        let pending_ui = pending_ui.clone();
        let path = path.clone();

        app.window()
            .set_rendering_notifier(move |state, api| match state {
                slint::RenderingState::RenderingSetup => {
                    if let Ok(info) = gl::describe_api(&api) {
                        eprintln!("{info}");
                    }
                    match gl::GlVideo::setup(&api, player.mpv()) {
                        Ok(mut v) => {
                            // Appelé depuis un thread mpv : uniquement un flag atomique
                            // (aucune API Slint ni mpv ici, cf. doc de set_update_callback).
                            let pending = frame_pending.clone();
                            v.render_ctx.set_update_callback(move || {
                                pending.store(true, Ordering::Release);
                            });

                            *pending_ui.borrow_mut() =
                                Some(UiUpdate::Frame(v.image.clone(), "lecture".into()));
                            *video.borrow_mut() = Some(v);

                            if let Err(e) = player.load(&path) {
                                eprintln!("chargement: {e:#}");
                            }
                        }
                        Err(e) => {
                            eprintln!("setup GL: {e:#}");
                            *pending_ui.borrow_mut() =
                                Some(UiUpdate::Status(format!("ERREUR GL: {e}")));
                        }
                    }
                }
                slint::RenderingState::BeforeRendering => {
                    if let Some(v) = video.borrow().as_ref() {
                        if frame_pending.swap(false, Ordering::AcqRel) {
                            let started = Instant::now();
                            if let Err(e) = v.render_ctx.render::<gl::LoaderCtx>(
                                v.fbo.0.get() as i32,
                                gl::VIDEO_W as i32,
                                gl::VIDEO_H as i32,
                                true,
                            ) {
                                eprintln!("render: {e}");
                            }
                            v.render_ctx.report_swap();
                            frames.fetch_add(1, Ordering::Relaxed);
                            render_us
                                .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
                        }
                    }
                }
                slint::RenderingState::RenderingTeardown => {
                    *video.borrow_mut() = None;
                }
                _ => {}
            })?;
    }

    // Le flag de frame est consommé par un timer côté UI : pas d'appel inter-thread.
    let redraw = slint::Timer::default();
    {
        let frame_pending = frame_pending.clone();
        let pending_ui = pending_ui.clone();
        let weak = app.as_weak();
        redraw.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(8),
            move || {
                let Some(app) = weak.upgrade() else { return };
                if let Some(update) = pending_ui.borrow_mut().take() {
                    match update {
                        UiUpdate::Frame(image, status) => {
                            app.set_frame(image);
                            app.set_status(status.into());
                        }
                        UiUpdate::Status(status) => app.set_status(status.into()),
                    }
                }
                if frame_pending.load(Ordering::Acquire) {
                    app.window().request_redraw();
                }
            },
        );
    }
    let _redraw = redraw;

    {
        let player = player.clone();
        let weak = app.as_weak();
        app.on_toggle_play(move || {
            let _ = player.set_paused(!player.paused());
            if let Some(app) = weak.upgrade() {
                app.set_playing(!player.paused());
            }
        });
    }

    {
        let player = player.clone();
        app.on_seek_to(move |fraction| {
            let (_, d) = player::sanitize_time(player.time_position(), player.duration());
            if d > 0.0 {
                let _ = player.seek_absolute((fraction as f64 * d).clamp(0.0, d));
            }
        });
    }

    // Instrumentation pour scripts/m0-measure.sh : un CSV par seconde.
    // M0_METRICS=/chemin.csv  et  M0_AUTOPAUSE_SEC=<secondes> (optionnel).
    let started_at = Instant::now();
    let mut metrics = match std::env::var("M0_METRICS") {
        Ok(path) => {
            let mut file = File::create(&path)?;
            writeln!(file, "t_s,fps,render_ms,position_s,duration_s,playing")?;
            Some(file)
        }
        Err(_) => None,
    };
    let autopause = std::env::var("M0_AUTOPAUSE_SEC")
        .ok()
        .and_then(|value| value.parse::<f64>().ok());

    let timer = slint::Timer::default();
    {
        let player = player.clone();
        let frames = frames.clone();
        let render_us = render_us.clone();
        let weak = app.as_weak();

        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(1),
            move || {
                let count = frames.swap(0, Ordering::Relaxed);
                let us = render_us.swap(0, Ordering::Relaxed);
                let fps = count as f32;
                let ms = if count > 0 {
                    us as f32 / count as f32 / 1000.0
                } else {
                    0.0
                };

                let (p, d) = player::sanitize_time(player.time_position(), player.duration());
                let progress = if d > 0.0 { (p / d) as f32 } else { 0.0 };

                if let Some(limit) = autopause {
                    if !player.paused() && p >= limit {
                        let _ = player.set_paused(true);
                    }
                }

                if let Some(file) = metrics.as_mut() {
                    let _ = writeln!(
                        file,
                        "{:.0},{:.0},{:.2},{:.2},{:.2},{}",
                        started_at.elapsed().as_secs_f64(),
                        fps,
                        ms,
                        p,
                        d,
                        u8::from(!player.paused())
                    );
                }

                if let Some(app) = weak.upgrade() {
                    app.set_fps(fps);
                    app.set_render_ms(ms);
                    app.set_playing(!player.paused());
                    app.set_position(p as f32);
                    app.set_duration(d as f32);
                    app.set_progress(progress);
                    app.set_progress_label(
                        format!(
                            "{}:{:02} / {}:{:02}",
                            (p / 60.0) as u32,
                            (p % 60.0) as u32,
                            (d / 60.0) as u32,
                            (d % 60.0) as u32
                        )
                        .into(),
                    );
                    app.set_metrics(format!("{fps:.0} fps · {ms:.1} ms/frame").into());
                }
            },
        );
    }
    let _timer = timer;

    app.run()?;
    Ok(())
}
