//! Logique pure du spike M0, sans aucune dépendance (ni libmpv, ni Slint, ni
//! système graphique). Elle se teste donc avec `cargo test` sur n'importe
//! quelle machine, y compris sans Wayland.

/// mpv renvoie NaN tant que le fichier n'est pas chargé, et peut renvoyer
/// une position supérieure à la durée après un seek en fin de fichier.
///
/// Retourne `(position, durée)` assainies : finies, positives, et position
/// bornée par la durée quand celle-ci est connue.
pub fn sanitize_time(position: f64, duration: f64) -> (f64, f64) {
    let d = if duration.is_finite() && duration > 0.0 {
        duration
    } else {
        0.0
    };
    let p = if position.is_finite() && position > 0.0 {
        if d > 0.0 {
            position.min(d)
        } else {
            position
        }
    } else {
        0.0
    };
    (p, d)
}

#[cfg(test)]
mod tests {
    use super::sanitize_time;

    #[test]
    fn nan_devient_zero() {
        assert_eq!(sanitize_time(f64::NAN, f64::NAN), (0.0, 0.0));
    }

    #[test]
    fn position_bornee_par_la_duree() {
        assert_eq!(sanitize_time(42.0, 30.0), (30.0, 30.0));
    }

    #[test]
    fn valeurs_normales_inchangees() {
        assert_eq!(sanitize_time(12.5, 60.0), (12.5, 60.0));
    }

    #[test]
    fn duree_inconnue_garde_la_position() {
        assert_eq!(sanitize_time(12.5, 0.0), (12.5, 0.0));
    }
}
