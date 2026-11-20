//! Session WEB par cookies : lecture d'un `cookies.txt` Netscape et signature
//! `SAPISIDHASH`, la seule forme que YouTube accepte en session navigateur.

use sha1::{Digest, Sha1};

#[derive(Debug)]
pub struct Cookies {
    pub sapisid: String,
    pub origin: String,
}

/// Lit le premier `SAPISID` (ou `__Secure-3PAPISID`) d'un export Netscape.
/// Les lignes `#HttpOnly_` sont du contenu, pas des commentaires.
pub fn parse_netscape(content: &str) -> Option<Cookies> {
    let mut sapisid = None;

    for line in content.lines() {
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 7 {
            continue;
        }
        let (domain, name, value) = (fields[0], fields[5], fields[6]);
        if !domain.contains("youtube.com") {
            continue;
        }
        if name == "SAPISID" {
            sapisid = Some(value.to_owned());
        } else if name == "__Secure-3PAPISID" && sapisid.is_none() {
            sapisid = Some(value.to_owned());
        }
    }

    sapisid.map(|sapisid| Cookies {
        sapisid,
        origin: "https://www.youtube.com".to_owned(),
    })
}

pub fn hash(sapisid: &str, timestamp: u64, origin: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("{timestamp} {sapisid} {origin}"));
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("SAPISIDHASH {timestamp}_{hex}")
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
# Netscape HTTP Cookie File
.youtube.com\tTRUE\t/\tTRUE\t1893456000\tSAPISID\tvaleur-secrete
#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1893456000\tLOGIN_INFO\tinterdit
.example.com\tTRUE\t/\tTRUE\t0\tSAPISID\tfuite
";

    #[test]
    fn extrait_sapisid_du_domaine_youtube() {
        let cookies = parse_netscape(FILE).expect("SAPISID présent");
        assert_eq!(cookies.sapisid, "valeur-secrete");
        assert_eq!(cookies.origin, "https://www.youtube.com");
    }

    #[test]
    fn accepte_le_cookie_http_only_et_le_repli_3p() {
        let file = "#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t0\t__Secure-3PAPISID\tvaleur\n";
        assert_eq!(
            parse_netscape(file).map(|cookies| cookies.sapisid),
            Some("valeur".to_owned())
        );
    }

    #[test]
    fn refuse_un_fichier_sans_sapisid_youtube() {
        assert!(parse_netscape("").is_none());
        assert!(parse_netscape(".example.com\tTRUE\t/\tTRUE\t0\tSAPISID\tfuite\n").is_none());
    }

    #[test]
    fn le_hash_a_la_forme_attendue() {
        let signed = hash("secret", 1_600_000_000, "https://www.youtube.com");
        let (scheme, rest) = signed.split_once(' ').expect("schéma puis empreinte");
        assert_eq!(scheme, "SAPISIDHASH");
        let (timestamp, digest) = rest.split_once('_').expect("horodatage puis empreinte");
        assert_eq!(timestamp, "1600000000");
        assert_eq!(digest.len(), 40);
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(
            hash("secret", 1_600_000_000, "https://www.youtube.com"),
            hash("secret", 1_600_000_001, "https://www.youtube.com")
        );
    }
}
