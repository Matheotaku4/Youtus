//! Progression vers YouTube : identifiant de lecture (`cpn`), paramètres des
//! beacons et machine d'état. Aucune I/O ici : les pings sont décidés par des
//! fonctions pures, le worker se charge de les envoyer.

/// Un `cpn` identifie **une** lecture du début à la fin ; un seek n'en crée
/// pas, une nouvelle lecture oui.
pub const CPN_CHARS: usize = 16;

/// Cadence des `videostatsWatchtimeUrl` : 30 s de position **nouvelle**.
pub const WATCHTIME_INTERVAL_MS: u64 = 30_000;

/// `atrUrl` part une fois la cinquième seconde atteinte.
pub const ATR_AFTER_MS: u64 = 5_000;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Encode 96 bits en base64url sans remplissage : 16 caractères `[A-Za-z0-9_-]`,
/// exactement le format attendu pour `cpn`.
pub fn encode_cpn(bytes: &[u8; 12]) -> String {
    let mut encoded = String::with_capacity(CPN_CHARS);
    for chunk in bytes.chunks_exact(3) {
        let triple = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        for shift in [18, 12, 6, 0] {
            encoded.push(ALPHABET[(triple >> shift) as usize & 0x3f] as char);
        }
    }
    encoded
}

/// Ajoute des paramètres à une URL signée sans écraser sa query d'origine.
pub fn with_params(base: &str, params: &[(&str, String)]) -> String {
    let mut url = base.to_owned();
    let mut separator = if url.contains('?') { '&' } else { '?' };
    for (name, value) in params {
        url.push(separator);
        separator = '&';
        url.push_str(name);
        url.push('=');
        url.push_str(value);
    }
    url
}

/// Paramètres de `videostatsPlaybackUrl`, envoyé au premier instant de lecture.
pub fn playback_params(cpn: &str) -> Vec<(&'static str, String)> {
    vec![
        ("cpn", cpn.to_owned()),
        ("ver", "2".to_owned()),
        ("cplayer", "UNIPLAYER".to_owned()),
    ]
}

/// Paramètres de `atrUrl` : le `cpn` suffit.
pub fn atr_params(cpn: &str) -> Vec<(&'static str, String)> {
    vec![("cpn", cpn.to_owned())]
}

/// Paramètres de `videostatsWatchtimeUrl`. `last` produit le ping de clôture
/// (`final=1&state=paused`) : c'est le seul qui part même sans nouvelle position.
pub fn watchtime_params(cpn: &str, position_ms: u64, last: bool) -> Vec<(&'static str, String)> {
    let seconds = position_ms / 1000;
    let mut params = vec![
        ("cpn", cpn.to_owned()),
        ("st", "0".to_owned()),
        ("et", seconds.to_string()),
        ("cmt", seconds.to_string()),
        ("state", if last { "paused" } else { "playing" }.to_owned()),
    ];
    if last {
        params.push(("final", "1".to_owned()));
    }
    params
}

/// Ce qu'il reste à envoyer pour une position observée.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ping {
    Playback,
    Atr,
    Watchtime { position_ms: u64, last: bool },
}

/// Décide quels pings envoyer. Le worker lui donne chaque position remontée par
/// l'UI (toutes les 5 s) et une clôture à la fermeture de la lecture.
#[derive(Clone, Debug)]
pub struct ProgressTracker {
    cpn: String,
    playback_sent: bool,
    atr_sent: bool,
    reported_ms: u64,
}

impl ProgressTracker {
    pub fn new(cpn: impl Into<String>) -> Self {
        Self {
            cpn: cpn.into(),
            playback_sent: false,
            atr_sent: false,
            reported_ms: 0,
        }
    }

    pub fn cpn(&self) -> &str {
        &self.cpn
    }

    pub fn observe(&mut self, position_ms: u64) -> Vec<Ping> {
        let mut pings = Vec::new();
        if !self.playback_sent {
            self.playback_sent = true;
            pings.push(Ping::Playback);
        }
        if !self.atr_sent && position_ms >= ATR_AFTER_MS {
            self.atr_sent = true;
            pings.push(Ping::Atr);
        }
        if position_ms >= self.reported_ms.saturating_add(WATCHTIME_INTERVAL_MS) {
            self.reported_ms = position_ms;
            pings.push(Ping::Watchtime {
                position_ms,
                last: false,
            });
        }
        pings
    }

    /// Ping de clôture : toujours émis, même si la position n'a pas bougé,
    /// pour que YouTube enregistre la fin de la lecture.
    pub fn finish(&mut self, position_ms: u64) -> Vec<Ping> {
        self.reported_ms = self.reported_ms.max(position_ms);
        vec![Ping::Watchtime {
            position_ms,
            last: true,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_cpn_fait_seize_caracteres_base64url() {
        let cpn = encode_cpn(&[0; 12]);
        assert_eq!(cpn, "AAAAAAAAAAAAAAAA");
        assert_eq!(encode_cpn(&[0xff; 12]), "________________");
        let cpn = encode_cpn(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(cpn.chars().count(), CPN_CHARS);
        assert!(cpn
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'));
    }

    #[test]
    fn deux_cpn_differents_donnent_des_chaines_differentes() {
        assert_ne!(encode_cpn(&[1; 12]), encode_cpn(&[2; 12]));
    }

    #[test]
    fn les_parametres_respectent_une_query_existante() {
        let url = with_params("https://example.test/a?x=1&sig=abc", &[("cpn", "CPN".to_owned())]);
        assert_eq!(url, "https://example.test/a?x=1&sig=abc&cpn=CPN");
        let url = with_params("https://example.test/a", &[("cpn", "CPN".to_owned())]);
        assert_eq!(url, "https://example.test/a?cpn=CPN");
    }

    #[test]
    fn le_ping_de_cloture_est_toujours_final() {
        let params = watchtime_params("CPN", 42_500, true);
        assert!(params.contains(&("final", "1".to_owned())));
        assert!(params.contains(&("state", "paused".to_owned())));
        assert!(params.contains(&("et", "42".to_owned())));
        assert!(params.contains(&("cmt", "42".to_owned())));
        assert!(!watchtime_params("CPN", 42_500, false)
            .iter()
            .any(|(name, _)| *name == "final"));
    }

    #[test]
    fn le_tracker_envoie_playback_puis_atr_puis_watchtime() {
        let mut tracker = ProgressTracker::new("CPN");
        assert_eq!(tracker.observe(0), vec![Ping::Playback]);
        assert_eq!(tracker.observe(4_000), Vec::new());
        assert_eq!(tracker.observe(5_000), vec![Ping::Atr]);
        assert_eq!(tracker.observe(20_000), Vec::new());
        assert_eq!(
            tracker.observe(30_000),
            vec![Ping::Watchtime {
                position_ms: 30_000,
                last: false
            }]
        );
        assert_eq!(tracker.observe(45_000), Vec::new());
        assert_eq!(
            tracker.observe(60_000),
            vec![Ping::Watchtime {
                position_ms: 60_000,
                last: false
            }]
        );
    }

    #[test]
    fn la_cloture_envoie_toujours_la_derniere_position() {
        let mut tracker = ProgressTracker::new("CPN");
        tracker.observe(0);
        assert_eq!(
            tracker.finish(12_000),
            vec![Ping::Watchtime {
                position_ms: 12_000,
                last: true
            }]
        );
        assert_eq!(
            tracker.finish(12_000),
            vec![Ping::Watchtime {
                position_ms: 12_000,
                last: true
            }]
        );
    }
}
