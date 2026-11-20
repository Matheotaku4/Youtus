use std::time::Duration;

use serde_json::{json, Map, Value};
use youtus_core::{Feed, FeedKind, VideoId};

use crate::auth::AuthStrategy;
use crate::parse::{parse_feed, parse_like_status, LikeStatus};
use crate::profile::{Profile, CHROME_UA, WEB_CLIENT_VERSION};

const BASE: &str = "https://www.youtube.com/youtubei/v1";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("authentification expirée (HTTP 401 sur /{endpoint}) : reconnecte-toi")]
    AuthExpired { endpoint: String },

    #[error("HTTP {status} sur /{endpoint} : {message}")]
    Http {
        status: u16,
        endpoint: String,
        message: String,
    },

    #[error("transport : {0}")]
    Transport(String),

    #[error("réponse JSON illisible : {0}")]
    Json(#[from] serde_json::Error),

    #[error("réponse inattendue : {0}")]
    Unexpected(String),
}

impl Error {
    /// Vrai quand l'utilisateur doit refaire le device flow ou réimporter ses
    /// cookies, et pas seulement réessayer.
    pub fn needs_reauth(&self) -> bool {
        matches!(self, Self::AuthExpired { .. })
    }
}

pub struct Client {
    agent: ureq::Agent,
    profile: Profile,
    auth: AuthStrategy,
    visitor: Option<String>,
}

impl Client {
    pub fn new(profile: Profile, auth: AuthStrategy) -> Self {
        let agent = ureq::AgentBuilder::new()
            .user_agent(profile.user_agent)
            .timeout(Duration::from_secs(30))
            .build();
        Self {
            agent,
            profile,
            auth,
            visitor: None,
        }
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    pub fn auth(&self) -> &AuthStrategy {
        &self.auth
    }

    pub fn set_auth(&mut self, auth: AuthStrategy) {
        self.auth = auth;
    }

    pub fn set_visitor_data(&mut self, visitor: impl Into<String>) {
        self.visitor = Some(visitor.into());
    }

    pub fn visitor_data(&self) -> Option<&str> {
        self.visitor.as_deref()
    }

    /// Récupère un `visitorData` anonyme. Indispensable aux requêtes
    /// authentifiées : un appel signé sans session visiteur est traité comme
    /// une requête invalide par InnerTube.
    pub fn bootstrap_visitor(&mut self) -> Result<(), Error> {
        if self.visitor.is_some() {
            return Ok(());
        }

        let agent = ureq::AgentBuilder::new()
            .user_agent(CHROME_UA)
            .timeout(Duration::from_secs(20))
            .build();
        let body = json!({
            "context": { "client": {
                "clientName": "WEB",
                "clientVersion": WEB_CLIENT_VERSION,
                "hl": "fr",
                "gl": "FR",
            }},
            "browseId": "FEwhat_to_watch",
        });

        let response = agent
            .post(&format!("{BASE}/browse?prettyPrint=false"))
            .set("Content-Type", "application/json")
            .set("X-YouTube-Client-Name", "1")
            .set("X-YouTube-Client-Version", WEB_CLIENT_VERSION)
            .send_json(body)
            .map_err(|error| match error {
                ureq::Error::Status(status, response) => Error::Http {
                    status,
                    endpoint: "browse".to_owned(),
                    message: snippet(&response.into_string().unwrap_or_default()),
                },
                other => Error::Transport(other.to_string()),
            })?;

        let text = response
            .into_string()
            .map_err(|error| Error::Transport(error.to_string()))?;
        let value: Value = serde_json::from_str(&text)?;
        let visitor = value
            .pointer("/responseContext/visitorData")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Unexpected("visitorData absent de /browse".to_owned()))?;
        self.visitor = Some(visitor.to_owned());
        Ok(())
    }

    pub fn post(&mut self, endpoint: &str, mut body: Value) -> Result<Value, Error> {
        if let Value::Object(map) = &mut body {
            map.insert("context".to_owned(), self.context());
        }

        let url = format!("{BASE}/{endpoint}?prettyPrint=false");
        let mut request = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json")
            .set("X-YouTube-Client-Name", self.profile.header_name)
            .set("X-YouTube-Client-Version", &self.profile.client_version);

        if self.profile.is_web() {
            request = request
                .set("Origin", "https://www.youtube.com")
                .set("Referer", "https://www.youtube.com/");
        }
        if let Some(visitor) = &self.visitor {
            request = request.set("X-Goog-Visitor-Id", visitor);
        }
        request = self.auth.apply(request);

        let response = match request.send_json(body) {
            Ok(response) => response,
            Err(ureq::Error::Status(401, _)) => {
                return Err(Error::AuthExpired {
                    endpoint: endpoint.to_owned(),
                })
            }
            Err(ureq::Error::Status(status, response)) => {
                let text = response.into_string().unwrap_or_default();
                return Err(Error::Http {
                    status,
                    endpoint: endpoint.to_owned(),
                    message: snippet(&text),
                });
            }
            Err(error) => return Err(Error::Transport(error.to_string())),
        };

        let text = response
            .into_string()
            .map_err(|error| Error::Transport(error.to_string()))?;
        let value: Value = serde_json::from_str(&text)?;
        self.learn_visitor(&value);
        Ok(value)
    }

    pub fn browse(&mut self, kind: &FeedKind) -> Result<Feed, Error> {
        self.browse_id(kind.browse_id())
    }

    pub fn browse_id(&mut self, browse_id: &str) -> Result<Feed, Error> {
        let value = self.post("browse", json!({ "browseId": browse_id }))?;
        Ok(parse_feed(&value))
    }

    pub fn browse_more(&mut self, continuation: &str) -> Result<Feed, Error> {
        let value = self.post("next", json!({ "continuation": continuation }))?;
        Ok(parse_feed(&value))
    }

    /// Pose un like. Un 200 sans `likeStatus` renvoie `LikeStatus::Unknown` :
    /// l'absence de preuve n'est pas une preuve.
    pub fn like(&mut self, video: &VideoId) -> Result<LikeStatus, Error> {
        let value = self.post(
            "like/like",
            json!({ "target": { "videoId": video.as_str() } }),
        )?;
        Ok(parse_like_status(&value))
    }

    pub fn unlike(&mut self, video: &VideoId) -> Result<(), Error> {
        self.post(
            "like/removelike",
            json!({ "target": { "videoId": video.as_str() } }),
        )?;
        Ok(())
    }

    pub fn agent(&self) -> &ureq::Agent {
        &self.agent
    }

    fn context(&self) -> Value {
        let mut client = Map::new();
        client.insert("clientName".to_owned(), json!(self.profile.client_name));
        client.insert(
            "clientVersion".to_owned(),
            json!(self.profile.client_version.as_str()),
        );
        client.insert("hl".to_owned(), json!("fr"));
        client.insert("gl".to_owned(), json!("FR"));
        if let Some(platform) = self.profile.platform {
            client.insert("platform".to_owned(), json!(platform));
        }
        if let Some(visitor) = &self.visitor {
            client.insert("visitorData".to_owned(), json!(visitor));
        }
        json!({ "client": Value::Object(client) })
    }

    fn learn_visitor(&mut self, value: &Value) {
        if self.visitor.is_some() {
            return;
        }
        if let Some(visitor) = value
            .pointer("/responseContext/visitorData")
            .and_then(Value::as_str)
        {
            self.visitor = Some(visitor.to_owned());
        }
    }
}

fn snippet(text: &str) -> String {
    text.chars().take(280).collect::<String>().replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_contexte_porte_le_profil_et_le_visiteur() {
        let mut client = Client::new(Profile::tvhtml5(), AuthStrategy::Anonymous);
        client.set_visitor_data("V1");
        let context = client.context();
        assert_eq!(context["client"]["clientName"], "TVHTML5");
        assert_eq!(context["client"]["platform"], "TV");
        assert_eq!(context["client"]["visitorData"], "V1");
        assert_eq!(context["client"]["hl"], "fr");
    }

    #[test]
    fn le_contexte_web_n_a_pas_de_plateforme() {
        let client = Client::new(Profile::web(), AuthStrategy::Anonymous);
        assert!(client.context()["client"].get("platform").is_none());
    }

    #[test]
    fn les_erreurs_d_authentification_demandent_une_reconnexion() {
        let expired = Error::AuthExpired {
            endpoint: "browse".to_owned(),
        };
        assert!(expired.needs_reauth());

        let http = Error::Http {
            status: 500,
            endpoint: "browse".to_owned(),
            message: String::new(),
        };
        assert!(!http.needs_reauth());
    }

    #[test]
    fn le_message_d_erreur_est_tronque_et_sur_une_ligne() {
        let long = "x".repeat(1000);
        assert!(snippet(&long).chars().count() <= 280);
        assert_eq!(snippet("a\nb"), "a b");
    }
}
