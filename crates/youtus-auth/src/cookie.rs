/// Cookies nécessaires à une session WEB : c'est la seule voie pour le flux
/// vidéo complet et la publication de commentaires (cf. S1).
#[derive(Clone, PartialEq, Eq)]
pub struct Cookies {
    pub sapisid: String,
    pub origin: String,
}

impl std::fmt::Debug for Cookies {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Cookies")
            .field("sapisid", &"••••")
            .field("origin", &self.origin)
            .finish()
    }
}

const ORIGIN: &str = "https://www.youtube.com";

/// Lit un `cookies.txt` au format Netscape (7 colonnes séparées par des
/// tabulations). Les lignes de commentaire et les cookies d'autres domaines
/// sont ignorés.
pub fn parse_netscape_cookies(content: &str) -> Option<Cookies> {
    let mut sapisid = None;

    for line in content.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut fields = line.split('\t');
        let (Some(domain), Some(_flag), Some(_path), Some(_secure), Some(_expiry), Some(name), Some(value)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };

        if !is_youtube_domain(domain) || value.is_empty() {
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
        origin: ORIGIN.to_owned(),
    })
}

fn is_youtube_domain(domain: &str) -> bool {
    let domain = domain.trim_start_matches('.');
    domain == "youtube.com" || domain == "www.youtube.com" || domain == "google.com"
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
# Netscape HTTP Cookie File
.youtube.com\tTRUE\t/\tTRUE\t1893456000\tSAPISID\tvaleur-secrete
.google.com\tTRUE\t/\tTRUE\t1893456000\tSID\tautre
.youtube.com\tTRUE\t/\tTRUE\t1893456000\t__Secure-3PAPISID\trepli
";

    #[test]
    fn extrait_sapisid() {
        let cookies = parse_netscape_cookies(FILE).expect("SAPISID présent");
        assert_eq!(cookies.sapisid, "valeur-secrete");
        assert_eq!(cookies.origin, "https://www.youtube.com");
    }

    #[test]
    fn retombe_sur_le_cookie_secure() {
        let content = ".youtube.com\tTRUE\t/\tTRUE\t0\t__Secure-3PAPISID\trepli\n";
        assert_eq!(
            parse_netscape_cookies(content).map(|cookies| cookies.sapisid),
            Some("repli".to_owned())
        );
    }

    #[test]
    fn ignore_les_autres_domaines() {
        let content = ".example.com\tTRUE\t/\tTRUE\t0\tSAPISID\tfuite\n";
        assert!(parse_netscape_cookies(content).is_none());
    }

    #[test]
    fn un_fichier_vide_ou_binaire_ne_panique_pas() {
        assert!(parse_netscape_cookies("").is_none());
        assert!(parse_netscape_cookies("n'importe quoi\n\n").is_none());
    }

    #[test]
    fn le_debug_ne_divulgue_pas_le_cookie() {
        let cookies = parse_netscape_cookies(FILE).unwrap();
        let text = format!("{cookies:?}");
        assert!(!text.contains("valeur-secrete"), "{text}");
    }
}
