//! Lecture vidéo de Youtus : résolution du flux par yt-dlp et rendu libmpv
//! dans une texture OpenGL fournie par l'application.
//!
//! Aucune dépendance à Slint : la couche UI récupère le chargeur OpenGL du
//! renderer et construit elle-même l'image à partir de la texture.

pub mod mpv;
pub mod ytdlp;

pub use mpv::{GlVideo, LoaderCtx, MpvPlayer, VIDEO_HEIGHT, VIDEO_WIDTH};
pub use ytdlp::{resolve, Error as ResolveError, ResolvedStream};
