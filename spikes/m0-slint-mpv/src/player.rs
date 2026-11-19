use anyhow::Result;

/// mpv renvoie NaN tant que le fichier n'est pas chargé, et peut renvoyer
/// une position supérieure à la durée après un seek en fin de fichier.
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

pub struct MpvPlayer {
    mpv: &'static libmpv2::Mpv,
}

impl MpvPlayer {
    pub fn new() -> Result<Self> {
        let mpv = libmpv2::Mpv::with_initializer(|init| {
            for (key, value) in [
                ("vo", "libmpv"),
                ("hwdec", "auto-safe"),
                ("osc", "no"),
                ("osd-level", "0"),
                ("input-default-bindings", "no"),
                ("input-vo-keyboard", "no"),
                ("keep-open", "yes"),
                ("ytdl", "no"),
                ("config", "no"),
                ("video-timing-offset", "0"),
                ("terminal", "no"),
                ("audio-display", "no"),
            ] {
                init.set_option(key, value)?;
            }
            Ok(())
        })?;
        Ok(Self {
            mpv: Box::leak(Box::new(mpv)),
        })
    }

    pub fn mpv(&self) -> &'static libmpv2::Mpv {
        self.mpv
    }

    pub fn load(&self, path: &str) -> Result<()> {
        self.mpv.command("loadfile", &[path, "replace"])?;
        Ok(())
    }

    pub fn set_paused(&self, paused: bool) -> Result<()> {
        self.mpv.set_property("pause", paused)?;
        Ok(())
    }

    pub fn paused(&self) -> bool {
        self.mpv.get_property::<bool>("pause").unwrap_or(false)
    }

    pub fn time_position(&self) -> f64 {
        self.mpv.get_property::<f64>("time-pos").unwrap_or(f64::NAN)
    }

    pub fn duration(&self) -> f64 {
        self.mpv.get_property::<f64>("duration").unwrap_or(f64::NAN)
    }

    pub fn seek_absolute(&self, secs: f64) -> Result<()> {
        self.mpv
            .command("seek", &[&format!("{secs:.3}"), "absolute"])?;
        Ok(())
    }
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
