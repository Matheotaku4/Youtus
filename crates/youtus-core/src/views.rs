/// Lit un nombre de vues localisé (`1 234 vues`, `1,2 M de vues`, `12 k`).
///
/// YouTube rend le texte localisé : l'espace fine et l'espace insécable
/// servent de séparateurs de milliers en français, la virgule de décimale, et
/// le suffixe peut être `M`, `million` ou `k`. Renvoie `None` quand aucun
/// nombre ne peut être lu (« Aucune vue »).
pub fn parse_view_count(text: &str) -> Option<u64> {
    let normalized = text.trim().to_lowercase();
    let mut digits = String::new();
    let mut rest = normalized.as_str();

    for (index, character) in normalized.char_indices() {
        let numeric = character.is_ascii_digit()
            || character == ','
            || character == '.'
            || character == ' '
            || character == '\u{a0}'
            || character == '\u{202f}';
        if numeric {
            digits.push(character);
        } else if !digits.is_empty() || character.is_alphabetic() {
            rest = &normalized[index..];
            break;
        }
    }

    if digits.is_empty() {
        return None;
    }

    let mut parts = digits.split([' ', '\u{a0}', '\u{202f}']);
    let head = parts.next()?;
    let tail: Vec<&str> = parts.filter(|part| !part.is_empty()).collect();

    let multiplier = multiplier_of(rest)?;
    // Décimales : on garde le point. Séparateurs de milliers : on les jette.
    let mut number = match decimal_part(head) {
        Some(decimal) => decimal,
        None => head.chars().filter(char::is_ascii_digit).collect(),
    };

    for group in tail {
        number.extend(group.chars().filter(char::is_ascii_digit));
    }

    let value: f64 = number.replace(',', ".").parse().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }

    Some((value * multiplier as f64).round() as u64)
}

fn multiplier_of(rest: &str) -> Option<u64> {
    let word: String = rest
        .trim_start_matches(|character: char| !character.is_alphabetic())
        .chars()
        .take_while(|character: &char| character.is_alphabetic())
        .collect();

    if word.is_empty() {
        return Some(1);
    }
    if word.starts_with('m') {
        return Some(1_000_000);
    }
    if word.starts_with('k') {
        return Some(1_000);
    }
    // Une unité inconnue (« de vues », « views ») : le nombre reste tel quel.
    Some(1)
}

/// `1,2` ou `1.2` sont des décimales ; `1,234` et `1.234` des milliers.
fn decimal_part(head: &str) -> Option<String> {
    for separator in [',', '.'] {
        if let Some(index) = head.find(separator) {
            let after = &head[index + 1..];
            if after.len() <= 2 && after.bytes().all(|byte| byte.is_ascii_digit()) {
                return Some(format!("{}.{}", &head[..index], after));
            }
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_les_nombres_francais() {
        assert_eq!(parse_view_count("1,2 M de vues"), Some(1_200_000));
        assert_eq!(parse_view_count("1,2 million de vues"), Some(1_200_000));
        assert_eq!(parse_view_count("1 234 567 vues"), Some(1_234_567));
        assert_eq!(parse_view_count("4 500 vues"), Some(4_500));
        assert_eq!(parse_view_count("12 k de vues"), Some(12_000));
    }

    #[test]
    fn lit_les_nombres_anglais() {
        assert_eq!(parse_view_count("1.2M views"), Some(1_200_000));
        assert_eq!(parse_view_count("1,234 views"), Some(1_234));
        assert_eq!(parse_view_count("856 views"), Some(856));
    }

    #[test]
    fn supporte_les_espaces_unicode() {
        assert_eq!(parse_view_count("1\u{a0}234\u{202f}567 vues"), Some(1_234_567));
    }

    #[test]
    fn refuse_ce_qui_n_a_pas_de_nombre() {
        assert_eq!(parse_view_count("Aucune vue"), None);
        assert_eq!(parse_view_count(""), None);
        assert_eq!(parse_view_count("il y a 1 jour"), None);
    }
}
