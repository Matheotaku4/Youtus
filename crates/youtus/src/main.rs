//! Point de composition : la seule crate qui connaît à la fois l'UI, le réseau
//! et le disque. Tout ce qui bloque vit dans `worker` ; ce fichier ne fait que
//! traduire des événements en propriétés Slint.

mod config;
mod worker;

use std::rc::Rc;
use std::time::Duration;

use slint::Timer;
use youtus_core::FeedKind;
use youtus_ui::App;

use config::Config;
use worker::Event;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|argument| argument == "-h" || argument == "--help") {
        print!("{}", config::USAGE);
        return Ok(());
    }
    if args.iter().any(|argument| argument == "-V" || argument == "--version") {
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
    app.on_command(move |command| {
        let command = match command {
            youtus_ui::Command::Navigate(page) => match feed_kind(&page) {
                Some(kind) => worker::Command::Load(kind),
                None => return,
            },
            youtus_ui::Command::Retry => worker::Command::Retry,
            youtus_ui::Command::SignIn => worker::Command::SignIn,
            youtus_ui::Command::Play(id) => worker::Command::Play(id),
        };
        let _ = commands.send(command);
    });

    let ui = Rc::clone(&app);
    let mut generation = 0u64;
    let timer = Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(80),
        move || {
            while let Ok(event) = events.try_recv() {
                apply(&ui, event, &mut generation);
            }
        },
    );

    app.run()?;
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

fn apply(app: &App, event: Event, generation: &mut u64) {
    match event {
        Event::Loading { page } => {
            app.set_page_title(&page);
            app.set_loading(true);
            app.set_error(None);
        }
        Event::Feed {
            generation: feed_generation,
            videos,
        } => {
            *generation = feed_generation;
            app.set_videos(&videos);
        }
        Event::Thumbnail {
            generation: thumbnail_generation,
            index,
            width,
            height,
            rgba,
        } => {
            if thumbnail_generation != *generation {
                return;
            }
            if let Some(image) = youtus_ui::image_from_rgba(&rgba, width, height) {
                app.set_thumbnail(index, image);
            }
        }
        Event::Error(message) => {
            app.set_loading(false);
            app.set_error(Some(&message));
        }
        Event::Notice(message) => app.set_status(&message),
        Event::SignedOut => {
            app.set_loading(false);
            app.set_error(None);
            app.set_signed_out(true);
        }
        Event::SignedIn(label) => {
            app.set_signed_out(false);
            app.set_user_code("");
            app.set_status(&label);
        }
        Event::DeviceCode {
            user_code,
            verification_url,
        } => {
            app.set_signed_out(true);
            app.set_user_code(&user_code);
            app.set_verification_url(&verification_url);
            app.set_status("En attente de l'autorisation…");
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
}
