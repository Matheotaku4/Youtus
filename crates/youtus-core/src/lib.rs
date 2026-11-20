//! Modèles purs de Youtus : aucun réseau, aucune UI, aucun accès disque.
//!
//! Les autres crates dépendent de celle-ci, jamais l'inverse.

pub mod model;
pub mod progress;
pub mod time;
pub mod views;

pub use model::{Feed, FeedKind, Video, VideoId};
pub use progress::{encode_cpn, with_params};
pub use time::{format_duration, parse_duration, sanitize_time};
pub use views::parse_view_count;
