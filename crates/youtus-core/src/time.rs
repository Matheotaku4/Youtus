use std::time::Duration;

/// Formate une durée comme YouTube : `2:03`, `1:02:03`.
pub fn format_duration(duration: Duration) -> String {
    let total = duration.as_secs();
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Lit `25:35`, `1:02:03` ou `2:03:04:05` (heures > 24).
pub fn parse_duration(text: &str) -> Option<Duration> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let mut seconds = 0u64;
    let mut parts = 0usize;
    for part in text.split(':') {
        let part = part.trim();
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let value: u64 = part.parse().ok()?;
        if parts > 0 && value >= 60 {
            return None;
        }
        seconds = seconds.checked_mul(60)?.checked_add(value)?;
        parts += 1;
    }

    (parts >= 2).then(|| Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formate_minutes_et_secondes() {
        assert_eq!(format_duration(Duration::from_secs(123)), "2:03");
        assert_eq!(format_duration(Duration::from_secs(9)), "0:09");
    }

    #[test]
    fn formate_les_heures() {
        assert_eq!(format_duration(Duration::from_secs(3723)), "1:02:03");
        assert_eq!(format_duration(Duration::from_secs(360_000)), "100:00:00");
    }

    #[test]
    fn lit_les_durees_youtube() {
        assert_eq!(parse_duration("25:35"), Some(Duration::from_secs(1535)));
        assert_eq!(parse_duration("1:02:03"), Some(Duration::from_secs(3723)));
        assert_eq!(parse_duration("0:09"), Some(Duration::from_secs(9)));
    }

    #[test]
    fn refuse_les_durees_mal_formees() {
        assert_eq!(parse_duration("25"), None);
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("1:99"), None);
        assert_eq!(parse_duration("1:2a"), None);
        assert_eq!(parse_duration("1:"), None);
    }

    #[test]
    fn aller_retour() {
        for seconds in [0, 9, 59, 60, 599, 3600, 3723, 86_400] {
            let text = format_duration(Duration::from_secs(seconds));
            assert_eq!(parse_duration(&text), Some(Duration::from_secs(seconds)), "{text}");
        }
    }
}
