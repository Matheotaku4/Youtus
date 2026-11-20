//! Client InnerTube de Youtus : profils, stratégies d'authentification et
//! lecture des réponses `browse` / `next`.
//!
//! Le profil et l'authentification sont **indépendants** : S1 a montré qu'un
//! jeton est lié à un type de client (200 en TVHTML5, 400 en WEB) et que le
//! client TV ne rend ni flux vidéo ni paramètres de commentaire. L'appelant
//! choisit donc le couple (profil, `AuthStrategy`) par capacité.

pub mod auth;
pub mod client;
pub mod parse;
pub mod player;
pub mod profile;

pub use auth::{sapisid_hash, AuthStrategy};
pub use client::{Client, Error};
pub use parse::{parse_feed, parse_like_status, LikeStatus};
pub use player::{PlayerResponse, Tracking};
pub use profile::{Profile, Family};
