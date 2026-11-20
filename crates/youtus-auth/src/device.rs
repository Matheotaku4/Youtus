use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

const DEVICE_CODE_URL: &str = "https://oauth2.googleapis.com/device/code";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/youtube";

#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: Option<String>,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Credentials")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "••••"),
            )
            .finish()
    }
}

impl Credentials {
    pub fn new(client_id: impl Into<String>, client_secret: Option<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret,
        }
    }

    pub fn from_json_path(path: &Path) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path).map_err(Error::Io)?;
        let value: Value = serde_json::from_str(&text)?;
        Self::from_json(&value)
    }

    /// Accepte le JSON téléchargé depuis la console (`installed`, `web`, `tv`,
    /// `device`) comme un objet plat.
    pub fn from_json(value: &Value) -> Result<Self, Error> {
        let root = ["installed", "web", "tv", "device"]
            .iter()
            .find_map(|key| value.get(*key))
            .unwrap_or(value);

        let client_id = root
            .get("client_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| Error::Credentials("client_id absent du JSON".to_owned()))?;

        let client_secret = root
            .get("client_secret")
            .and_then(Value::as_str)
            .filter(|secret| !secret.is_empty())
            .map(str::to_owned);

        Ok(Self::new(client_id, client_secret))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("identifiants OAuth : {0}")]
    Credentials(String),

    #[error("réponse OAuth illisible : {0}")]
    Json(#[from] serde_json::Error),

    #[error("disque : {0}")]
    Io(#[from] std::io::Error),

    #[error("transport : {0}")]
    Transport(String),

    #[error("client OAuth refusé ({0}) : vérifie client_id et client_secret")]
    InvalidClient(String),

    #[error("autorisation refusée par l'utilisateur")]
    AccessDenied,

    #[error("code expiré ou jeton révoqué (invalid_grant) : reconnecte-toi")]
    InvalidGrant,

    #[error("OAuth a renvoyé {0} : {1}")]
    OAuth(String, String),

    #[error("aucune autorisation reçue avant l'expiration du code")]
    Timeout,
}

pub fn request_device_code(credentials: &Credentials) -> Result<DeviceCode, Error> {
    let agent = ureq::agent();
    let request = agent.post(DEVICE_CODE_URL).set(
        "Content-Type",
        "application/x-www-form-urlencoded",
    );

    let mut form = format!("client_id={}&scope={}", credentials.client_id, SCOPE);
    if let Some(secret) = &credentials.client_secret {
        form.push_str(&format!("&client_secret={secret}"));
    }

    let response = request.send_string(&form).map_err(map_transport)?;
    let value = read_json(response)?;

    let device_code = string_field(&value, "device_code")?;
    let user_code = string_field(&value, "user_code")?;
    let verification_url = value
        .get("verification_url")
        .or_else(|| value.get("verification_uri"))
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Credentials("verification_url absent".to_owned()))?
        .to_owned();

    Ok(DeviceCode {
        device_code,
        user_code,
        verification_url,
        expires_in: value.get("expires_in").and_then(Value::as_u64).unwrap_or(1800),
        interval: value.get("interval").and_then(Value::as_u64).unwrap_or(5),
    })
}

pub fn poll_token(credentials: &Credentials, code: &DeviceCode) -> Result<TokenResponse, Error> {
    let deadline = Instant::now() + Duration::from_secs(code.expires_in);
    let mut interval = code.interval.max(1);

    loop {
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        std::thread::sleep(Duration::from_secs(interval));

        let mut form = format!(
            "client_id={}&device_code={}&grant_type=urn:ietf:params:oauth:grant-type:device_code",
            credentials.client_id, code.device_code
        );
        if let Some(secret) = &credentials.client_secret {
            form.push_str(&format!("&client_secret={secret}"));
        }

        match post_form(&form) {
            Ok(value) => return token_from(value),
            Err(error) => match error {
                PollError::Pending => continue,
                PollError::SlowDown => {
                    interval += 5;
                    continue;
                }
                PollError::Fatal(error) => return Err(error),
            },
        }
    }
}

pub fn refresh(credentials: &Credentials, refresh_token: &str) -> Result<TokenResponse, Error> {
    let mut form = format!(
        "client_id={}&refresh_token={refresh_token}&grant_type=refresh_token",
        credentials.client_id
    );
    if let Some(secret) = &credentials.client_secret {
        form.push_str(&format!("&client_secret={secret}"));
    }

    let value = post_form(&form).map_err(|error| match error {
        PollError::Fatal(error) => error,
        PollError::Pending | PollError::SlowDown => {
            Error::OAuth("refresh".to_owned(), "réponse transitoire inattendue".to_owned())
        }
    })?;

    token_from(value)
}

enum PollError {
    Pending,
    SlowDown,
    Fatal(Error),
}

fn post_form(form: &str) -> Result<Value, PollError> {
    let agent = ureq::agent();
    let response = agent
        .post(TOKEN_URL)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(form);

    match response {
        Ok(response) => read_json(response).map_err(PollError::Fatal),
        Err(ureq::Error::Status(_, response)) => {
            let body = response.into_string().unwrap_or_default();
            let value: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let code = value.get("error").and_then(Value::as_str).unwrap_or("");

            match code {
                "authorization_pending" => Err(PollError::Pending),
                "slow_down" => Err(PollError::SlowDown),
                "" => Err(PollError::Fatal(Error::OAuth(
                    "http".to_owned(),
                    body.chars().take(160).collect(),
                ))),
                other => Err(PollError::Fatal(classify(other, &value))),
            }
        }
        Err(error) => Err(PollError::Fatal(Error::Transport(error.to_string()))),
    }
}

fn classify(code: &str, value: &Value) -> Error {
    let description = value
        .get("error_description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();

    match code {
        "invalid_client" => Error::InvalidClient(description),
        "invalid_grant" => Error::InvalidGrant,
        "access_denied" => Error::AccessDenied,
        "expired_token" => Error::Timeout,
        other => Error::OAuth(other.to_owned(), description),
    }
}

fn token_from(value: Value) -> Result<TokenResponse, Error> {
    Ok(TokenResponse {
        access_token: string_field(&value, "access_token")?,
        refresh_token: value
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_owned),
        expires_in: value.get("expires_in").and_then(Value::as_u64).unwrap_or(3600),
    })
}

fn string_field(value: &Value, key: &str) -> Result<String, Error> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::Credentials(format!("{key} absent de la réponse")))
}

fn read_json(response: ureq::Response) -> Result<Value, Error> {
    let text = response
        .into_string()
        .map_err(|error| Error::Transport(error.to_string()))?;
    Ok(serde_json::from_str(&text)?)
}

fn map_transport(error: ureq::Error) -> Error {
    match error {
        ureq::Error::Status(status, response) => {
            let body = response.into_string().unwrap_or_default();
            let value: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let code = value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("http");
            classify(code, &value).with_status(status)
        }
        other => Error::Transport(other.to_string()),
    }
}

impl Error {
    fn with_status(self, status: u16) -> Self {
        match self {
            Error::OAuth(code, description) if description.is_empty() => {
                Error::OAuth(code, format!("HTTP {status}"))
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lit_le_json_de_la_console() {
        let value = json!({
            "installed": {
                "client_id": "123.apps.googleusercontent.com",
                "client_secret": "secret",
                "redirect_uris": ["http://localhost"]
            }
        });
        let credentials = Credentials::from_json(&value).unwrap();
        assert_eq!(credentials.client_id, "123.apps.googleusercontent.com");
        assert_eq!(credentials.client_secret.as_deref(), Some("secret"));
    }

    #[test]
    fn lit_un_json_plat_sans_secret() {
        let value = json!({ "client_id": "abc.apps.googleusercontent.com" });
        let credentials = Credentials::from_json(&value).unwrap();
        assert_eq!(credentials.client_secret, None);
    }

    #[test]
    fn refuse_un_json_sans_client_id() {
        assert!(Credentials::from_json(&json!({ "installed": {} })).is_err());
        assert!(Credentials::from_json(&json!({ "installé": {} })).is_err());
    }

    #[test]
    fn le_debug_ne_divulgue_pas_le_secret() {
        let credentials = Credentials::new("id", Some("SECRET".to_owned()));
        let text = format!("{credentials:?}");
        assert!(!text.contains("SECRET"), "{text}");
        assert!(text.contains("id"));
    }

    #[test]
    fn classe_les_erreurs_oauth() {
        assert!(matches!(
            classify("invalid_client", &json!({})),
            Error::InvalidClient(_)
        ));
        assert!(matches!(
            classify("invalid_grant", &json!({})),
            Error::InvalidGrant
        ));
        assert!(matches!(
            classify("access_denied", &json!({})),
            Error::AccessDenied
        ));
        assert!(matches!(
            classify("temporarily_unavailable", &json!({})),
            Error::OAuth(_, _)
        ));
    }

    #[test]
    fn extrait_verification_uri_comme_repli() {
        // Google renvoie `verification_url`, d'autres serveurs `verification_uri`.
        let value = json!({ "verification_uri": "https://example.com/device" });
        assert_eq!(
            value
                .get("verification_url")
                .or_else(|| value.get("verification_uri"))
                .and_then(Value::as_str),
            Some("https://example.com/device")
        );
    }
}
