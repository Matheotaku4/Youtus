//! Fenêtre Slint et pont vers l'application. Aucune I/O ici : l'UI émet des
//! `Command` et reçoit des données déjà chargées.

use std::rc::Rc;
use std::time::Duration;

use slint::{Model, ModelRc, VecModel};
use youtus_core::Video;

slint::include_modules!();

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Navigate(String),
    Retry,
    Play(String),
    SignIn,
}

pub struct App {
    window: MainWindow,
    cards: Rc<VecModel<VideoCard>>,
}

impl App {
    pub fn new() -> Result<Self, slint::PlatformError> {
        let window = MainWindow::new()?;
        let cards = Rc::new(VecModel::default());
        window.set_cards(ModelRc::from(cards.clone()));
        Ok(Self { window, cards })
    }

    pub fn window(&self) -> &MainWindow {
        &self.window
    }

    pub fn run(&self) -> Result<(), slint::PlatformError> {
        self.window.run()
    }

    pub fn set_videos(&self, videos: &[Video]) {
        self.cards
            .set_vec(videos.iter().map(card_from).collect::<Vec<_>>());
        self.window.set_loading(false);
        self.window.set_has_error(false);
    }

    pub fn set_thumbnail(&self, index: usize, image: slint::Image) {
        if let Some(mut card) = self.cards.row_data(index) {
            card.thumbnail = image;
            self.cards.set_row_data(index, card);
        }
    }

    pub fn card_count(&self) -> usize {
        self.cards.row_count()
    }

    pub fn set_page_title(&self, title: &str) {
        self.window.set_page_title(title.into());
    }

    pub fn set_status(&self, status: &str) {
        self.window.set_status(status.into());
    }

    pub fn set_loading(&self, loading: bool) {
        self.window.set_loading(loading);
    }

    pub fn set_error(&self, error: Option<&str>) {
        match error {
            Some(message) => {
                self.window.set_has_error(true);
                self.window.set_error_text(message.into());
                self.window.set_loading(false);
            }
            None => {
                self.window.set_has_error(false);
                self.window.set_error_text("".into());
            }
        }
    }

    pub fn set_signed_out(&self, signed_out: bool) {
        self.window.set_signed_out(signed_out);
    }

    pub fn set_user_code(&self, code: &str) {
        self.window.set_user_code(code.into());
    }

    pub fn set_verification_url(&self, url: &str) {
        self.window.set_verification_url(url.into());
    }

    pub fn on_command(&self, handler: impl Fn(Command) + 'static) {
        let handler = Rc::new(handler);

        let navigate = handler.clone();
        self.window.on_navigate(move |page| {
            navigate(Command::Navigate(page.to_string()));
        });

        let retry = handler.clone();
        self.window.on_retry(move || {
            retry(Command::Retry);
        });

        let sign_in = handler.clone();
        self.window.on_sign_in(move || {
            sign_in(Command::SignIn);
        });

        self.window.on_play(move |id| {
            handler(Command::Play(id.to_string()));
        });
    }
}

pub fn card_from(video: &Video) -> VideoCard {
    VideoCard {
        id: video.id.as_str().into(),
        title: video.title.as_str().into(),
        channel: video.channel.clone().unwrap_or_default().into(),
        meta: meta_line(video),
        thumbnail: slint::Image::default(),
    }
}

fn meta_line(video: &Video) -> slint::SharedString {
    let mut parts = Vec::new();
    if let Some(views) = video.views {
        parts.push(format_count(views));
    }
    if let Some(duration) = video.duration {
        parts.push(format_duration(duration));
    }
    if let Some(published) = &video.published {
        parts.push(published.clone());
    }
    parts.join(" · ").into()
}

fn format_duration(duration: Duration) -> String {
    youtus_core::format_duration(duration)
}

fn format_count(views: u64) -> String {
    match views {
        0..=999 => format!("{views} vues"),
        1_000..=999_999 => format!("{:.1} k vues", views as f64 / 1_000.0),
        _ => format!("{:.1} M de vues", views as f64 / 1_000_000.0),
    }
}

/// Construit une image Slint depuis des octets RGBA. À appeler sur le thread UI.
pub fn image_from_rgba(bytes: &[u8], width: u32, height: u32) -> Option<slint::Image> {
    let expected = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    if bytes.len() != expected {
        return None;
    }
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        bytes, width, height,
    );
    Some(slint::Image::from_rgba8(buffer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use youtus_core::VideoId;

    fn video() -> Video {
        let mut video = Video::new(VideoId::new("dQw4w9WgXcQ").unwrap(), "Titre");
        video.channel = Some("Chaîne".to_owned());
        video.views = Some(1_234_567);
        video.duration = Some(Duration::from_secs(213));
        video.published = Some("il y a 1 jour".to_owned());
        video
    }

    #[test]
    fn la_carte_reprend_les_champs() {
        let card = card_from(&video());
        assert_eq!(card.id, "dQw4w9WgXcQ");
        assert_eq!(card.title, "Titre");
        assert_eq!(card.channel, "Chaîne");
        assert_eq!(card.meta, "1.2 M de vues · 3:33 · il y a 1 jour");
    }

    #[test]
    fn une_video_sans_metadonnees_reste_lisible() {
        let video = Video::new(VideoId::new("abcdefghijk").unwrap(), "Seul");
        assert_eq!(card_from(&video).meta, "");
    }

    #[test]
    fn refuse_une_image_de_taille_incoherente() {
        assert!(image_from_rgba(&[0; 64], 4, 4).is_some());
        assert!(image_from_rgba(&[0; 63], 4, 4).is_none());
        assert!(image_from_rgba(&[0; 4], 0, 0).is_none());
    }
}
