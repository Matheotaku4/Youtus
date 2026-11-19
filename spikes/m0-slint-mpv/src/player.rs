use anyhow::Result;

/// Réexport de la logique pure (crate `m0-core`, testable sans libmpv).
pub use m0_core::sanitize_time;

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

    pub fn eof_reached(&self) -> bool {
        self.mpv
            .get_property::<bool>("eof-reached")
            .unwrap_or(false)
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
