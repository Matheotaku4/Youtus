//! `/player` en contexte WEB : c'est le seul client qui rend un flux jouable
//! et `playbackTracking` (le client TV répond `UNPLAYABLE`, mesuré en S1).
//!
//! Le `signatureTimestamp` doit être celui du player JS **courant** : sans lui,
//! YouTube omet `playbackTracking` ; avec celui d'un autre client, il répond
//! « La page doit être actualisée ».

use serde_json::{json, Value};

use crate::client::{Client, Error};

const WATCH: &str = "https://www.youtube.com/watch";

/// URLs signées des trois beacons de lecture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tracking {
    pub playback: String,
    pub watchtime: String,
    pub atr: Option<String>,
}

/// Réponse utile de `/player` : l'état de lecture et le tracking éventuel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerResponse {
    pub status: String,
    pub tracking: Option<Tracking>,
}

impl PlayerResponse {
    pub fn from_json(value: &Value) -> Self {
        Self {
            status: value
                .pointer("/playabilityStatus/status")
                .and_then(Value::as_str)
                .unwrap_or("inconnu")
                .to_owned(),
            tracking: parse_tracking(value),
        }
    }
}

/// Lit `playbackTracking` sans supposer que les trois URLs sont présentes :
/// `atrUrl` manque souvent, les deux autres sont indispensables.
pub fn parse_tracking(value: &Value) -> Option<Tracking> {
    let tracking = value.get("playbackTracking")?;
    let url = |key: &str| {
        tracking
            .get(key)
            .and_then(|entry| entry.get("baseUrl"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    Some(Tracking {
        playback: url("videostatsPlaybackUrl")?,
        watchtime: url("videostatsWatchtimeUrl")?,
        atr: url("atrUrl"),
    })
}

/// Extrait `jsUrl` du HTML de la page watch (`base.js` pour WEB).
pub fn extract_js_url(html: &str) -> Option<String> {
    for marker in ["\"jsUrl\":\"", "\"PLAYER_JS_URL\":\""] {
        if let Some(start) = html.find(marker) {
            let rest = &html[start + marker.len()..];
            let end = rest.find('"')?;
            return Some(rest[..end].replace("\\/", "/"));
        }
    }
    None
}

/// Le `signatureTimestamp` est une constante du player JS, pas une donnée de la
/// réponse watch : il faut télécharger le JS et y chercher la première valeur.
pub fn extract_sts(js: &str) -> Option<u32> {
    for marker in ["signatureTimestamp:", "signatureTimestamp="] {
        let mut cursor = 0;
        while let Some(offset) = js[cursor..].find(marker) {
            let start = cursor + offset + marker.len();
            let digits: String = js[start..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Ok(sts) = digits.parse() {
                return Some(sts);
            }
            cursor = start;
        }
    }
    None
}

impl Client {
    /// Télécharge le player JS courant et en extrait le `signatureTimestamp`.
    pub fn signature_timestamp(&self, video_id: &str) -> Result<u32, Error> {
        let html = self
            .agent()
            .get(&format!("{WATCH}?v={video_id}"))
            .call()
            .map_err(|error| transport("watch", error))?
            .into_string()
            .map_err(|error| Error::Transport(error.to_string()))?;

        let js_url = extract_js_url(&html)
            .ok_or_else(|| Error::Unexpected("jsUrl absent de la page watch".to_owned()))?;
        let js_url = if js_url.starts_with("http") {
            js_url
        } else {
            format!("https://www.youtube.com{js_url}")
        };

        let js = self
            .agent()
            .get(&js_url)
            .call()
            .map_err(|error| transport("player JS", error))?
            .into_string()
            .map_err(|error| Error::Transport(error.to_string()))?;

        extract_sts(&js)
            .ok_or_else(|| Error::Unexpected(format!("signatureTimestamp absent de {js_url}")))
    }

    /// `/player` WEB avec le `sts` courant, puis lecture du tracking.
    pub fn player(&mut self, video_id: &str) -> Result<PlayerResponse, Error> {
        let sts = self.signature_timestamp(video_id)?;
        let value = self.post(
            "player",
            json!({
                "videoId": video_id,
                "contentCheckOk": true,
                "racyCheckOk": true,
                "playbackContext": {
                    "contentPlaybackContext": { "signatureTimestamp": sts }
                },
            }),
        )?;
        Ok(PlayerResponse::from_json(&value))
    }
}

fn transport(step: &str, error: ureq::Error) -> Error {
    match error {
        ureq::Error::Status(status, response) => Error::Http {
            status,
            endpoint: step.to_owned(),
            message: response.into_string().unwrap_or_default(),
        },
        other => Error::Transport(format!("{step} : {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrait_le_js_url_echappe() {
        let html = r#"{"jsUrl":"\/s\/player\/abc\/player_ias.vflset\/fr_FR\/base.js"}"#;
        assert_eq!(
            extract_js_url(html).as_deref(),
            Some("/s/player/abc/player_ias.vflset/fr_FR/base.js")
        );
        assert_eq!(extract_js_url("<html></html>"), None);
    }

    #[test]
    fn extrait_le_signature_timestamp() {
        let js = r#"a.signatureTimestamp:20774,b"#;
        assert_eq!(extract_sts(js), Some(20774));
        assert_eq!(extract_sts(r#"x="signatureTimestamp=19555""#), Some(19555));
        assert_eq!(extract_sts("rien"), None);
    }

    #[test]
    fn lit_les_trois_urls_de_tracking() {
        let value = json!({
            "playbackTracking": {
                "videostatsPlaybackUrl": { "baseUrl": "https://s/playback?x=1" },
                "videostatsWatchtimeUrl": { "baseUrl": "https://s/watchtime?x=1" },
                "atrUrl": { "baseUrl": "https://s/atr?x=1" }
            }
        });
        let tracking = parse_tracking(&value).expect("tracking complet");
        assert_eq!(tracking.playback, "https://s/playback?x=1");
        assert_eq!(tracking.watchtime, "https://s/watchtime?x=1");
        assert_eq!(tracking.atr.as_deref(), Some("https://s/atr?x=1"));
    }

    #[test]
    fn sans_les_deux_urls_indispensables_pas_de_tracking() {
        let value = json!({
            "playbackTracking": { "videostatsPlaybackUrl": { "baseUrl": "https://s/p" } }
        });
        assert_eq!(parse_tracking(&value), None);
        assert_eq!(parse_tracking(&json!({})), None);
    }

    #[test]
    fn la_reponse_porte_le_statut_de_lecture() {
        let value = json!({
            "playabilityStatus": { "status": "UNPLAYABLE", "reason": "x" },
            "playbackTracking": {}
        });
        let response = PlayerResponse::from_json(&value);
        assert_eq!(response.status, "UNPLAYABLE");
        assert!(response.tracking.is_none());
        assert_eq!(PlayerResponse::from_json(&json!({})).status, "inconnu");
    }
}
