use sha1::{Digest, Sha1};

/// Comment signer une requête InnerTube.
///
/// `Bearer` couvre les feeds OAuth (client TV) ; `Sapisid` couvre ce que le
/// client TV ne sait pas faire (flux vidéo, `createCommentParams`).
#[derive(Clone, PartialEq, Eq)]
pub enum AuthStrategy {
    Anonymous,
    Bearer { access_token: String },
    Sapisid { sapisid: String, origin: String },
}

impl std::fmt::Debug for AuthStrategy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anonymous => formatter.write_str("Anonymous"),
            Self::Bearer { .. } => formatter.write_str("Bearer { •••• }"),
            Self::Sapisid { origin, .. } => {
                write!(formatter, "Sapisid {{ ••••, origin: {origin} }}")
            }
        }
    }
}

impl AuthStrategy {
    pub fn is_authenticated(&self) -> bool {
        !matches!(self, Self::Anonymous)
    }

    /// Applique les en-têtes d'authentification à une requête déjà construite.
    pub fn apply(&self, request: ureq::Request) -> ureq::Request {
        self.apply_at(request, unix_now())
    }

    pub fn apply_at(&self, request: ureq::Request, timestamp: u64) -> ureq::Request {
        match self {
            Self::Anonymous => request,
            Self::Bearer { access_token } => {
                request.set("Authorization", &format!("Bearer {access_token}"))
            }
            Self::Sapisid { sapisid, origin } => request
                .set(
                    "Cookie",
                    &format!("SAPISID={sapisid}; __Secure-3PAPISID={sapisid}"),
                )
                .set("Authorization", &sapisid_hash(sapisid, timestamp, origin)),
        }
    }
}

/// `SAPISIDHASH <horodatage>_<sha1(horodatage sapisid origine)>`.
pub fn sapisid_hash(sapisid: &str, timestamp: u64, origin: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("{timestamp} {sapisid} {origin}"));
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("SAPISIDHASH {timestamp}_{hex}")
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_hash_a_la_forme_attendue() {
        let hash = sapisid_hash("secret", 1_600_000_000, "https://www.youtube.com");
        let (scheme, rest) = hash.split_once(' ').expect("un espace sépare le schéma");
        assert_eq!(scheme, "SAPISIDHASH");
        let (timestamp, digest) = rest.split_once('_').expect("horodatage puis empreinte");
        assert_eq!(timestamp, "1600000000");
        assert_eq!(digest.len(), 40);
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn le_hash_depend_de_l_horodatage() {
        let a = sapisid_hash("secret", 1_600_000_000, "https://www.youtube.com");
        let b = sapisid_hash("secret", 1_600_000_001, "https://www.youtube.com");
        assert_ne!(a, b);
    }

    #[test]
    fn le_debug_ne_divulgue_ni_jeton_ni_cookie() {
        let bearer = AuthStrategy::Bearer {
            access_token: "ya29.SECRET".to_owned(),
        };
        let text = format!("{bearer:?}");
        assert!(!text.contains("SECRET"), "{text}");

        let sapisid = AuthStrategy::Sapisid {
            sapisid: "SECRET".to_owned(),
            origin: "https://www.youtube.com".to_owned(),
        };
        let text = format!("{sapisid:?}");
        assert!(!text.contains("SECRET"), "{text}");
    }

    #[test]
    fn une_session_anonyme_n_est_pas_authentifiee() {
        assert!(!AuthStrategy::Anonymous.is_authenticated());
        assert!(AuthStrategy::Bearer {
            access_token: "x".to_owned()
        }
        .is_authenticated());
    }
}
