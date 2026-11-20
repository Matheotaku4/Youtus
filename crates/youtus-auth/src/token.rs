use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Ce qui est persisté : jamais l'access token (il expire), jamais le secret du
/// client. Le refresh token seul suffit à reprendre une session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredToken {
    pub client_id: String,
    pub refresh_token: String,
}

impl StoredToken {
    pub fn new(client_id: impl Into<String>, refresh_token: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            refresh_token: refresh_token.into(),
        }
    }
}

pub fn data_dir() -> PathBuf {
    if let Ok(path) = std::env::var("XDG_DATA_HOME") {
        if !path.is_empty() {
            return PathBuf::from(path).join("youtus");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home).join(".local/share/youtus");
        }
    }
    PathBuf::from(".youtus")
}

pub fn default_token_path() -> PathBuf {
    data_dir().join("oauth.json")
}

pub fn load_token(path: &Path) -> Option<StoredToken> {
    let text = std::fs::read_to_string(path).ok()?;
    let token: StoredToken = serde_json::from_str(&text).ok()?;
    (!token.refresh_token.is_empty() && !token.client_id.is_empty()).then_some(token)
}

/// Écrit le jeton en 0600 via un fichier temporaire renommé : une coupure au
/// mauvais moment ne laisse pas un JSON tronqué à la place d'un jeton valide.
pub fn save_token(path: &Path, token: &StoredToken) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let temporary = path.with_extension("tmp");
    let body = serde_json::to_vec_pretty(token)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    std::fs::write(&temporary, body)?;
    restrict(&temporary)?;
    std::fs::rename(&temporary, path)?;
    restrict(path)?;
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let unique = format!(
            "youtus-test-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique).join("oauth.json")
    }

    #[test]
    fn ecrit_puis_relit() {
        let path = temp_path("roundtrip");
        let token = StoredToken::new("client", "refresh");
        save_token(&path, &token).unwrap();
        assert_eq!(load_token(&path), Some(token));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[cfg(unix)]
    #[test]
    fn le_fichier_est_prive() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_path("perms");
        save_token(&path, &StoredToken::new("client", "refresh")).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn un_fichier_absent_ou_invalide_ne_panique_pas() {
        assert_eq!(load_token(Path::new("/nonexistent/youtus/oauth.json")), None);
    }

    #[test]
    fn le_chemin_par_defaut_est_sous_youtus() {
        let path = default_token_path();
        assert!(path.ends_with("youtus/oauth.json"), "{path:?}");
    }
}
