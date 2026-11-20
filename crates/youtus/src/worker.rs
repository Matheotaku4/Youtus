//! Worker unique : tout le réseau et tout le disque passent par ici. L'UI ne
//! reçoit que des messages déjà décodés, jetons et cookies compris dans le
//! `Debug` masqué des crates bibliothèques.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver, Sender};
use youtus_auth::{
    load_token, parse_netscape_cookies, poll_token, refresh, request_device_code, save_token,
    Credentials, StoredToken,
};
use youtus_core::{Feed, FeedKind, Video, VideoId};
use youtus_store::Store;
use youtus_yt::{AuthStrategy, Client, Profile};

use crate::config::Config;

const THUMB_WIDTH: u32 = 320;
const THUMB_HEIGHT: u32 = 180;
const MAX_HEIGHT: u32 = 720;

#[derive(Clone, Debug)]
pub enum Command {
    Load(FeedKind),
    Retry,
    SignIn,
    Play { id: String, title: String },
    Progress {
        video_id: String,
        position_ms: u64,
        duration_ms: u64,
    },
}

#[derive(Debug)]
pub enum Event {
    Loading { page: String },
    Feed { generation: u64, videos: Vec<Video> },
    Thumbnail {
        generation: u64,
        index: usize,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    Error(String),
    Notice(String),
    SignedOut,
    SignedIn(String),
    DeviceCode {
        user_code: String,
        verification_url: String,
    },
    Playback {
        video_id: String,
        title: String,
        url: String,
        audio_url: Option<String>,
        headers: Vec<(String, String)>,
        start_at: Option<f64>,
    },
}

pub fn spawn(config: Config) -> (Sender<Command>, Receiver<Event>) {
    let (commands_tx, commands_rx) = unbounded();
    let (events_tx, events_rx) = unbounded();

    std::thread::Builder::new()
        .name("youtus-net".to_owned())
        .spawn(move || {
            let mut worker = Worker::new(config, events_tx);
            worker.run(&commands_rx);
        })
        .expect("thread réseau");

    (commands_tx, events_rx)
}

enum Session {
    OAuth {
        client: Client,
        credentials: Credentials,
        refresh_token: String,
    },
    Cookies {
        client: Client,
    },
}

impl Session {
    fn client(&mut self) -> &mut Client {
        match self {
            Self::OAuth { client, .. } => client,
            Self::Cookies { client } => client,
        }
    }
}

struct Failure {
    message: String,
    needs_signin: bool,
}

struct ThumbTask {
    generation: u64,
    index: usize,
    url: String,
}

struct Worker {
    config: Config,
    events: Sender<Event>,
    session: Option<Session>,
    store: Option<Store>,
    page: FeedKind,
    generation: Arc<AtomicU64>,
    thumbnails: Sender<ThumbTask>,
}

impl Worker {
    fn new(config: Config, events: Sender<Event>) -> Self {
        let store = match Store::open(&config.db_path) {
            Ok(store) => Some(store),
            Err(error) => {
                let _ = events.send(Event::Notice(format!(
                    "Base locale indisponible ({error}) : réglages non enregistrés."
                )));
                None
            }
        };

        let generation = Arc::new(AtomicU64::new(0));
        let thumbnails = start_thumbnails(events.clone(), Arc::clone(&generation));

        let mut worker = Self {
            config,
            events,
            session: None,
            store,
            page: FeedKind::Home,
            generation,
            thumbnails,
        };

        match worker.open_session() {
            Ok(Some(session)) => {
                worker.session = Some(session);
                let _ = worker.events.send(Event::SignedIn("Session restaurée.".to_owned()));
            }
            Ok(None) => {}
            Err(message) => {
                let _ = worker.events.send(Event::Error(message));
            }
        }

        worker
    }

    fn run(&mut self, commands: &Receiver<Command>) {
        if self.session.is_some() {
            let page = self.last_page();
            self.load(page);
        } else {
            let _ = self.events.send(Event::SignedOut);
        }

        while let Ok(command) = commands.recv() {
            match command {
                Command::Load(kind) => self.load(kind),
                Command::Retry => {
                    let page = self.page.clone();
                    self.load(page);
                }
                Command::SignIn => self.sign_in(),
                Command::Play { id, title } => self.start_playback(id, title),
                Command::Progress {
                    video_id,
                    position_ms,
                    duration_ms,
                } => self.save_progress(&video_id, position_ms, duration_ms),
            }
        }
    }

    /// Rouvre la page quittée la dernière fois, sinon l'accueil.
    fn last_page(&self) -> FeedKind {
        let stored = self.store.as_ref().and_then(|store| store.setting("last_page").ok().flatten());
        match stored.as_deref() {
            Some("FEsubscriptions") => FeedKind::Subscriptions,
            Some("FEhistory") => FeedKind::History,
            Some("FEtrending") => FeedKind::Trending,
            _ => FeedKind::Home,
        }
    }

    fn open_session(&self) -> Result<Option<Session>, String> {
        if let Some(token) = load_token(&self.config.token_path) {
            let credentials =
                Credentials::from_json_path(&self.config.credentials_path).map_err(|error| {
                    format!(
                        "client OAuth illisible dans {} : {error}\nRelance avec --client-secret-file <fichier.json>.",
                        self.config.credentials_path.display()
                    )
                })?;

            let refreshed = refresh(&credentials, &token.refresh_token)
                .map_err(|error| format!("session OAuth : {error}"))?;

            let mut client = Client::new(
                Profile::tvhtml5(),
                AuthStrategy::Bearer {
                    access_token: refreshed.access_token,
                },
            );
            client
                .bootstrap_visitor()
                .map_err(|error| format!("session visiteur : {error}"))?;

            return Ok(Some(Session::OAuth {
                client,
                credentials,
                refresh_token: refreshed
                    .refresh_token
                    .unwrap_or(token.refresh_token),
            }));
        }

        if let Some(path) = &self.config.cookies_path {
            let text = std::fs::read_to_string(path)
                .map_err(|error| format!("cookies illisibles ({}): {error}", path.display()))?;
            let cookies = parse_netscape_cookies(&text).ok_or_else(|| {
                format!(
                    "aucun SAPISID dans {} : exporte les cookies YouTube au format Netscape.",
                    path.display()
                )
            })?;

            let mut client = Client::new(
                Profile::web(),
                AuthStrategy::Sapisid {
                    sapisid: cookies.sapisid,
                    origin: cookies.origin,
                },
            );
            client
                .bootstrap_visitor()
                .map_err(|error| format!("session visiteur : {error}"))?;

            return Ok(Some(Session::Cookies { client }));
        }

        Ok(None)
    }

    fn sign_in(&mut self) {
        if self.session.is_some() {
            let _ = self.events.send(Event::Notice("Déjà connecté.".to_owned()));
            return;
        }

        let credentials = match Credentials::from_json_path(&self.config.credentials_path) {
            Ok(credentials) => credentials,
            Err(error) => {
                let _ = self.events.send(Event::Error(format!(
                    "Aucun client OAuth lisible dans {} : {error}\nRelance l'app avec --client-secret-file <fichier.json>.",
                    self.config.credentials_path.display()
                )));
                return;
            }
        };

        let code = match request_device_code(&credentials) {
            Ok(code) => code,
            Err(error) => {
                let _ = self.events.send(Event::Error(format!("OAuth : {error}")));
                return;
            }
        };

        let _ = self.events.send(Event::DeviceCode {
            user_code: code.user_code.clone(),
            verification_url: code.verification_url.clone(),
        });

        match poll_token(&credentials, &code) {
            Ok(token) => {
                if let Some(refresh_token) = &token.refresh_token {
                    let stored =
                        StoredToken::new(credentials.client_id.clone(), refresh_token.clone());
                    if let Err(error) = save_token(&self.config.token_path, &stored) {
                        let _ = self.events.send(Event::Notice(format!(
                            "Jeton utilisable mais non enregistré : {error}"
                        )));
                    }
                }

                let mut client = Client::new(
                    Profile::tvhtml5(),
                    AuthStrategy::Bearer {
                        access_token: token.access_token,
                    },
                );
                match client.bootstrap_visitor() {
                    Ok(()) => {
                        self.session = Some(Session::OAuth {
                            client,
                            credentials,
                            refresh_token: token.refresh_token.unwrap_or_default(),
                        });
                        let _ =
                            self.events.send(Event::SignedIn("Connecté à YouTube.".to_owned()));
                        self.load(FeedKind::Home);
                    }
                    Err(error) => {
                        let _ = self.events
                            .send(Event::Error(format!("session : {error}")));
                    }
                }
            }
            Err(error) => {
                let _ = self
                    .events
                    .send(Event::Error(format!("Connexion : {error}")));
            }
        }
    }

    /// Résout le flux puis renvoie la position de reprise lue en base.
    fn start_playback(&mut self, id: String, title: String) {
        if VideoId::new(id.clone()).is_none() {
            let _ = self
                .events
                .send(Event::Error(format!("identifiant vidéo invalide : {id}")));
            return;
        }

        let _ = self
            .events
            .send(Event::Notice(format!("Résolution du flux : {title}")));

        let resume_ms = self
            .store
            .as_ref()
            .and_then(|store| store.resume_position(&id).ok().flatten())
            .unwrap_or(0);

        match youtus_play::resolve(&id, MAX_HEIGHT, self.config.cookies_path.as_deref()) {
            Ok(stream) => {
                eprintln!(
                    "youtus: flux {} ({}p), audio séparé : {}",
                    stream.format_id,
                    stream.height.unwrap_or(0),
                    stream.audio_url.is_some()
                );
                let start_at = resume_seconds(resume_ms, stream.duration);
                let _ = self.events.send(Event::Playback {
                    video_id: id,
                    title,
                    url: stream.url,
                    audio_url: stream.audio_url,
                    headers: stream.http_headers,
                    start_at,
                });
            }
            Err(error) => {
                let _ = self
                    .events
                    .send(Event::Error(format!("Lecture impossible : {error}")));
            }
        }
    }

    fn save_progress(&mut self, video_id: &str, position_ms: u64, duration_ms: u64) {
        if let Some(store) = &self.store {
            if let Err(error) = store.save_playback(video_id, position_ms, duration_ms) {
                let _ = self
                    .events
                    .send(Event::Notice(format!("Position non enregistrée : {error}")));
            }
        }
    }

    fn load(&mut self, kind: FeedKind) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        if self.session.is_none() {
            let _ = self.events.send(Event::SignedOut);
            return;
        }

        self.page = kind.clone();
        let _ = self.events.send(Event::Loading {
            page: kind.label().to_owned(),
        });

        match self.browse(&kind) {
            Ok(feed) => self.show(generation, kind, feed),
            Err(failure) => {
                let _ = self.events.send(Event::Error(failure.message));
                if failure.needs_signin {
                    self.session = None;
                    let _ = self.events.send(Event::SignedOut);
                }
            }
        }
    }

    fn browse(&mut self, kind: &FeedKind) -> Result<Feed, Failure> {
        match self.browse_once(kind) {
            Ok(feed) => Ok(feed),
            Err(error) if error.needs_reauth() => {
                if let Err(message) = self.relogin() {
                    return Err(Failure {
                        message: format!("Session expirée ({message}) : reconnecte-toi."),
                        needs_signin: true,
                    });
                }
                self.browse_once(kind).map_err(|error| Failure {
                    message: error.to_string(),
                    needs_signin: error.needs_reauth(),
                })
            }
            Err(error) => Err(Failure {
                message: error.to_string(),
                needs_signin: false,
            }),
        }
    }

    fn browse_once(&mut self, kind: &FeedKind) -> Result<Feed, youtus_yt::Error> {
        match self.session.as_mut() {
            Some(session) => session.client().browse(kind),
            None => Err(youtus_yt::Error::Unexpected("non connecté".to_owned())),
        }
    }

    /// Un access token expire au bout d'une heure : on le renouvelle sans
    /// renvoyer l'utilisateur au device flow.
    fn relogin(&mut self) -> Result<(), String> {
        let Some(Session::OAuth {
            client,
            credentials,
            refresh_token,
        }) = self.session.as_mut()
        else {
            return Err("session par cookies".to_owned());
        };

        let refreshed = refresh(credentials, refresh_token).map_err(|error| error.to_string())?;
        client.set_auth(AuthStrategy::Bearer {
            access_token: refreshed.access_token,
        });

        if let Some(new_token) = refreshed.refresh_token {
            let stored = StoredToken::new(credentials.client_id.clone(), new_token.clone());
            let _ = save_token(&self.config.token_path, &stored);
            *refresh_token = new_token;
        }

        Ok(())
    }

    fn show(&mut self, generation: u64, kind: FeedKind, feed: Feed) {
        let jobs: Vec<ThumbTask> = feed
            .items
            .iter()
            .enumerate()
            .map(|(index, video)| ThumbTask {
                generation,
                index,
                url: thumbnail_url(video),
            })
            .collect();

        if let Some(store) = &self.store {
            let _ = store.set_setting("last_page", kind.browse_id());
        }

        let _ = self.events.send(Event::Feed {
            generation,
            videos: feed.items,
        });

        for job in jobs {
            let _ = self.thumbnails.send(job);
        }
    }
}

/// Reprendre au début ou à la toute fin n'apporte rien : on ne restaure la
/// position qu'à partir de 5 s et tant qu'il reste au moins 5 s à voir.
fn resume_seconds(resume_ms: u64, duration: Option<f64>) -> Option<f64> {
    if resume_ms < 5_000 {
        return None;
    }
    let position = resume_ms as f64 / 1000.0;
    match duration {
        Some(duration) if duration > position + 5.0 => Some(position),
        _ => None,
    }
}

fn thumbnail_url(video: &Video) -> String {
    video.thumbnail.clone().unwrap_or_else(|| {
        format!(
            "https://i.ytimg.com/vi/{}/mqdefault.jpg",
            video.id.as_str()
        )
    })
}

fn start_thumbnails(events: Sender<Event>, generation: Arc<AtomicU64>) -> Sender<ThumbTask> {
    let (tasks_tx, tasks_rx) = unbounded::<ThumbTask>();

    for index in 0..2 {
        let tasks = tasks_rx.clone();
        let events = events.clone();
        let generation = Arc::clone(&generation);
        let _ = std::thread::Builder::new()
            .name(format!("youtus-thumb-{index}"))
            .spawn(move || {
                let agent = ureq::AgentBuilder::new()
                    .user_agent(youtus_yt::profile::CHROME_UA)
                    .timeout(Duration::from_secs(15))
                    .build();

                for task in tasks.iter() {
                    // Une miniature de l'ancienne page ne doit pas écraser une
                    // carte de la nouvelle : la génération fait foi.
                    if task.generation != generation.load(Ordering::Relaxed) {
                        continue;
                    }
                    if let Some((width, height, rgba)) = fetch_thumbnail(&agent, &task.url) {
                        let _ = events.send(Event::Thumbnail {
                            generation: task.generation,
                            index: task.index,
                            width,
                            height,
                            rgba,
                        });
                    }
                }
            });
    }

    tasks_tx
}

fn fetch_thumbnail(agent: &ureq::Agent, url: &str) -> Option<(u32, u32, Vec<u8>)> {
    use std::io::Read;

    // Une miniature fait quelques dizaines de kilo-octets : le plafond évite
    // qu'une réponse inattendue ne tienne tout le worker en mémoire.
    let mut bytes = Vec::new();
    agent
        .get(url)
        .call()
        .ok()?
        .into_reader()
        .take(4 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    let image = image::load_from_memory(&bytes).ok()?;
    let image = if image.width() > THUMB_WIDTH || image.height() > THUMB_HEIGHT {
        image.resize(
            THUMB_WIDTH,
            THUMB_HEIGHT,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some((width, height, rgba.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use youtus_core::VideoId;

    #[test]
    fn la_reprise_ignore_le_debut_et_la_fin() {
        assert_eq!(resume_seconds(0, Some(200.0)), None);
        assert_eq!(resume_seconds(4_000, Some(200.0)), None);
        assert_eq!(resume_seconds(42_000, Some(200.0)), Some(42.0));
        assert_eq!(resume_seconds(199_000, Some(200.0)), None);
        assert_eq!(resume_seconds(42_000, None), None);
    }

    #[test]
    fn la_miniature_retombe_sur_mqdefault() {
        let video = Video::new(VideoId::new("dQw4w9WgXcQ").unwrap(), "Titre");
        assert_eq!(
            thumbnail_url(&video),
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg"
        );
    }

    #[test]
    fn la_miniature_du_parseur_est_prioritaire() {
        let mut video = Video::new(VideoId::new("dQw4w9WgXcQ").unwrap(), "Titre");
        video.thumbnail = Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hq.jpg".to_owned());
        assert_eq!(
            thumbnail_url(&video),
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/hq.jpg"
        );
    }
}
