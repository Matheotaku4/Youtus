use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEVICE_CODE_URL: &str = "https://oauth2.googleapis.com/device/code";
const DEVICE_CODE_FALLBACK: &str = "https://www.youtube.com/o/oauth2/device/code";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/youtube";

pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub interval: u64,
    pub expires_in: u64,
}

pub struct Token {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct StoredToken {
    pub client_id: String,
    pub refresh_token: String,
}

pub fn default_token_path() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".local/share")
        });
    base.join("youtus").join("oauth.json")
}

pub fn load_token(path: &Path) -> Option<StoredToken> {
    let mut text = String::new();
    fs::File::open(path).ok()?.read_to_string(&mut text).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_token(path: &Path, token: &StoredToken) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("création de {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_string_pretty(token)?)
        .with_context(|| format!("écriture de {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn form_body(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-encoding strict (RFC 3986 non réservé).
fn encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn post_form(url: &str, pairs: &[(&str, &str)]) -> Result<Value> {
    let response = ureq::post(url)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&form_body(pairs));
    match response {
        Ok(resp) => Ok(resp.into_json()?),
        // Les erreurs OAuth (authorization_pending, …) arrivent en 4xx avec un corps JSON.
        Err(ureq::Error::Status(_, resp)) => Ok(resp.into_json().unwrap_or_else(|_| Value::Null)),
        Err(error) => Err(error.into()),
    }
}

pub fn request_device_code(client_id: &str) -> Result<DeviceCode> {
    let device_id = format!(
        "youtus-s1-{}",
        std::process::id()
    );
    let pairs = [
        ("client_id", client_id),
        ("scope", SCOPE),
        ("device_id", device_id.as_str()),
        ("device_model", "ytlr::"),
    ];

    let value = match post_form(DEVICE_CODE_URL, &pairs)? {
        Value::Null => post_form(DEVICE_CODE_FALLBACK, &pairs)?,
        value => value,
    };

    if let Some(error) = value.get("error").and_then(Value::as_str) {
        bail!(
            "refus du device code : {error} — {}",
            value
                .get("error_description")
                .and_then(Value::as_str)
                .unwrap_or("")
        );
    }

    let field = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("champ `{name}` absent de la réponse device code : {value}"))
    };

    Ok(DeviceCode {
        device_code: field("device_code")?,
        user_code: field("user_code")?,
        verification_url: value
            .get("verification_url")
            .or_else(|| value.get("verification_uri"))
            .and_then(Value::as_str)
            .unwrap_or("https://www.google.com/device")
            .to_owned(),
        interval: value.get("interval").and_then(Value::as_u64).unwrap_or(5),
        expires_in: value
            .get("expires_in")
            .and_then(Value::as_u64)
            .unwrap_or(300),
    })
}

pub fn poll_token(client_id: &str, code: &DeviceCode) -> Result<Token> {
    let deadline = Instant::now() + Duration::from_secs(code.expires_in);
    let mut interval = code.interval.max(1);

    loop {
        if Instant::now() >= deadline {
            bail!("code expiré avant autorisation");
        }
        std::thread::sleep(Duration::from_secs(interval));

        let value = post_form(
            TOKEN_URL,
            &[
                ("client_id", client_id),
                ("device_code", code.device_code.as_str()),
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:device_code",
                ),
            ],
        )?;

        if let Some(access) = value.get("access_token").and_then(Value::as_str) {
            return Ok(Token {
                access_token: access.to_owned(),
                refresh_token: value
                    .get("refresh_token")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            });
        }

        match value.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval += 1;
                continue;
            }
            Some(other) => bail!(
                "{}",
                describe_error(
                    other,
                    value
                        .get("error_description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                )
            ),
            None => bail!("réponse token inattendue : {value}"),
        }
    }
}

pub fn refresh(client_id: &str, refresh_token: &str) -> Result<Token> {
    let value = post_form(
        TOKEN_URL,
        &[
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ],
    )?;

    if let Some(access) = value.get("access_token").and_then(Value::as_str) {
        return Ok(Token {
            access_token: access.to_owned(),
            refresh_token: None,
        });
    }

    bail!(
        "refresh refusé : {} — {}",
        value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("inconnu"),
        value
            .get("error_description")
            .and_then(Value::as_str)
            .unwrap_or("")
    )
}

/// Traduit les erreurs OAuth qui ont une cause de configuration précise.
fn describe_error(code: &str, description: &str) -> String {
    if description.contains("client_secret") {
        return "le client OAuth utilisé est confidentiel (type « Application Web » ou \
                « Desktop » avec secret) : Google réclame un client_secret pour échanger le \
                device code. Le device flow exige un client public « TV et périphériques à \
                entrée limitée », qui n'a pas de secret ; recrée-en un de ce type et relance \
                avec son identifiant brut (sans suffixe .apps.googleusercontent.com en double)."
            .to_owned();
    }
    format!("autorisation refusée : {code} — {description}")
}

#[cfg(test)]
mod tests {
    use super::{describe_error, encode};

    #[test]
    fn explique_le_client_confidentiel() {
        let message = describe_error(
            "invalid_request",
            "Missing required parameter: client_secret",
        );
        assert!(message.contains("TV et périphériques à entrée limitée"));
        assert!(!message.contains("autorisation refusée"));
    }

    #[test]
    fn garde_les_autres_erreurs() {
        assert_eq!(
            describe_error("access_denied", "user denied"),
            "autorisation refusée : access_denied — user denied"
        );
    }

    #[test]
    fn encode_les_scopes() {
        assert_eq!(
            encode("https://www.googleapis.com/auth/youtube"),
            "https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fyoutube"
        );
    }

    #[test]
    fn laisse_les_non_reserves() {
        assert_eq!(encode("aZ09-._~"), "aZ09-._~");
    }
}
