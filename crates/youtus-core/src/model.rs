use std::fmt;
use std::str::FromStr;
use std::time::Duration;

/// Identifiant YouTube : exactement 11 caractères `[A-Za-z0-9_-]`.
///
/// Le type existe pour qu'un identifiant extrait d'une réponse JSON ne puisse
/// pas arriver jusqu'à une URL sans avoir été validé une fois.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VideoId(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidVideoId(pub String);

impl fmt::Display for InvalidVideoId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "identifiant vidéo invalide : {:?}", self.0)
    }
}

impl std::error::Error for InvalidVideoId {}

impl VideoId {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        Self::is_valid(&value).then_some(Self(value))
    }

    pub fn is_valid(value: &str) -> bool {
        value.len() == 11
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VideoId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for VideoId {
    type Err = InvalidVideoId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value).ok_or_else(|| InvalidVideoId(value.to_owned()))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Video {
    pub id: VideoId,
    pub title: String,
    pub channel: Option<String>,
    pub channel_id: Option<String>,
    pub duration: Option<Duration>,
    pub views: Option<u64>,
    pub published: Option<String>,
    pub thumbnail: Option<String>,
}

impl Video {
    pub fn new(id: VideoId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            channel: None,
            channel_id: None,
            duration: None,
            views: None,
            published: None,
            thumbnail: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feed {
    pub items: Vec<Video>,
    pub continuation: Option<String>,
}

impl Feed {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

/// Surfaces navigables qui partagent le même endpoint `browse`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedKind {
    Home,
    Subscriptions,
    History,
    Trending,
}

impl FeedKind {
    pub fn browse_id(&self) -> &'static str {
        match self {
            Self::Home => "FEwhat_to_watch",
            Self::Subscriptions => "FEsubscriptions",
            Self::History => "FEhistory",
            Self::Trending => "FEtrending",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Home => "Accueil",
            Self::Subscriptions => "Abonnements",
            Self::History => "Historique",
            Self::Trending => "Tendances",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepte_un_identifiant_valide() {
        let id = VideoId::new("dQw4w9WgXcQ").expect("identifiant valide");
        assert_eq!(id.as_str(), "dQw4w9WgXcQ");
        assert_eq!(id.to_string(), "dQw4w9WgXcQ");
    }

    #[test]
    fn refuse_un_identifiant_trop_court_ou_invalide() {
        assert!(VideoId::new("court").is_none());
        assert!(VideoId::new("dQw4w9WgXc ").is_none());
        assert!(VideoId::new("dQw4w9WgXc/").is_none());
        assert!("dQw4w9WgXc!" .parse::<VideoId>().is_err());
    }

    #[test]
    fn les_identifiants_se_comparent_par_valeur() {
        let a: VideoId = "dQw4w9WgXcQ".parse().unwrap();
        let b = VideoId::new("dQw4w9WgXcQ").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn chaque_surface_a_son_browse_id() {
        assert_eq!(FeedKind::Home.browse_id(), "FEwhat_to_watch");
        assert_eq!(FeedKind::Subscriptions.browse_id(), "FEsubscriptions");
        assert_eq!(FeedKind::History.browse_id(), "FEhistory");
        assert_eq!(FeedKind::Trending.browse_id(), "FEtrending");
    }
}
