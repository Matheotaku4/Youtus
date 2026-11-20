//! libmpv en mode API render : mpv dessine dans un FBO, l'UI affiche la texture.
//! `--wid` n'est jamais utilisé, il est impossible sous Wayland.

use std::ffi::{c_void, CStr, CString};
use std::num::NonZeroU32;

use anyhow::{anyhow, Result};
use glow::HasContext;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};

pub const VIDEO_WIDTH: u32 = 1280;
pub const VIDEO_HEIGHT: u32 = 720;

/// Contexte transmis à mpv pour résoudre les symboles OpenGL.
pub struct LoaderCtx {
    f: &'static (dyn Fn(&CStr) -> *const c_void + Send + Sync),
}

/// Signature exigée par `OpenGLInitParams::get_proc_address` (pointeur de
/// fonction, pas de `dyn Fn`).
fn get_proc(ctx: &LoaderCtx, name: &str) -> *mut c_void {
    let name = CString::new(name).expect("nom de symbole GL sans octet nul");
    (ctx.f)(name.as_c_str()) as *mut c_void
}

pub struct MpvPlayer {
    mpv: &'static libmpv2::Mpv,
}

impl MpvPlayer {
    pub fn new() -> Result<Self> {
        // `YOUTUS_MPV_LOG=1` rend la parole à mpv sur stderr : indispensable
        // pour diagnostiquer un flux que mpv refuse d'ouvrir en silence.
        let verbose = std::env::var_os("YOUTUS_MPV_LOG").is_some();
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
                ("terminal", if verbose { "yes" } else { "no" }),
                ("audio-display", "no"),
                ("cache", "yes"),
                ("demuxer-max-bytes", "256MiB"),
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

    /// Charge une URL résolue par yt-dlp. La reprise de position n'est pas
    /// passée ici : `start` est une option locale de fichier que mpv refuse
    /// après l'initialisation (PROPERTY_ERROR) ; l'UI seek dès que la durée
    /// est connue.
    pub fn load(&self, url: &str, headers: &[(String, String)]) -> Result<()> {
        // `http-header-fields` est une liste séparée par des virgules : un
        // en-tête dont la valeur contient une virgule (l'`Accept` de yt-dlp)
        // rendrait l'option invalide. On l'écarte, l'UA passe par sa propriété.
        let fields = headers
            .iter()
            .filter(|(name, value)| {
                !name.eq_ignore_ascii_case("accept") && !value.contains(',')
            })
            .map(|(name, value)| format!("{name}: {value}"))
            .collect::<Vec<_>>()
            .join(",");
        if !fields.is_empty() {
            self.mpv
                .set_property("http-header-fields", fields)
                .map_err(|error| anyhow!("http-header-fields : {error}"))?;
        }
        if let Some((_, agent)) = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
        {
            self.mpv
                .set_property("user-agent", agent.to_owned())
                .map_err(|error| anyhow!("user-agent : {error}"))?;
        }
        self.mpv
            .command("loadfile", &[url, "replace"])
            .map_err(|error| anyhow!("loadfile : {error}"))?;
        Ok(())
    }

    /// Ajoute la piste audio d'un flux DASH séparé, une fois le fichier chargé.
    pub fn add_audio(&self, url: &str) -> Result<()> {
        self.mpv
            .command("audio-add", &[url])
            .map_err(|error| anyhow!("audio-add : {error}"))?;
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        self.mpv.command("stop", &[])?;
        Ok(())
    }

    pub fn set_paused(&self, paused: bool) -> Result<()> {
        self.mpv.set_property("pause", paused)?;
        Ok(())
    }

    pub fn paused(&self) -> bool {
        self.mpv.get_property::<bool>("pause").unwrap_or(false)
    }

    pub fn position(&self) -> f64 {
        self.mpv.get_property::<f64>("time-pos").unwrap_or(f64::NAN)
    }

    pub fn duration(&self) -> f64 {
        self.mpv.get_property::<f64>("duration").unwrap_or(f64::NAN)
    }

    pub fn eof_reached(&self) -> bool {
        self.mpv
            .get_property::<bool>("eof-reached")
            .unwrap_or(false)
    }

    pub fn seek(&self, seconds: f64) -> Result<()> {
        self.mpv
            .command("seek", &[&format!("{seconds:.3}"), "absolute"])?;
        Ok(())
    }

    pub fn set_volume(&self, volume: f64) -> Result<()> {
        self.mpv.set_property("volume", volume)?;
        Ok(())
    }
}

/// FBO + texture + contexte de rendu mpv. Doit être détruit avant le contexte
/// mpv et avant le contexte OpenGL de la fenêtre.
pub struct GlVideo {
    pub texture: NonZeroU32,
    pub fbo: glow::Framebuffer,
    pub render_ctx: RenderContext<'static>,
    _gl: glow::Context,
}

impl GlVideo {
    /// `loader` vient du renderer Slint ; il reste valide aussi longtemps que
    /// la fenêtre, qui survit au contexte de rendu créé ici.
    pub fn setup(
        loader: &dyn Fn(&CStr) -> *const c_void,
        mpv: &'static libmpv2::Mpv,
    ) -> Result<Self> {
        // SAFETY: cf. doc ci-dessus — le pointeur est détenu par la fenêtre
        // Slint, gardée vivante pendant toute la durée du contexte de rendu.
        let loader: &'static (dyn Fn(&CStr) -> *const c_void + Send + Sync) =
            unsafe { std::mem::transmute(loader) };

        let gl = unsafe { glow::Context::from_loader_function_cstr(|s| loader(s)) };

        let (texture, fbo) = unsafe {
            let texture = gl
                .create_texture()
                .map_err(|error| anyhow!("création de la texture GL : {error}"))?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                VIDEO_WIDTH as i32,
                VIDEO_HEIGHT as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            for parameter in [glow::TEXTURE_MIN_FILTER, glow::TEXTURE_MAG_FILTER] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, parameter, glow::LINEAR as i32);
            }
            for parameter in [glow::TEXTURE_WRAP_S, glow::TEXTURE_WRAP_T] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, parameter, glow::CLAMP_TO_EDGE as i32);
            }

            let fbo = gl
                .create_framebuffer()
                .map_err(|error| anyhow!("création du FBO : {error}"))?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            (texture, fbo)
        };

        let render_ctx = mpv.create_render_context(vec![
            RenderParam::ApiType(RenderParamApiType::OpenGl),
            RenderParam::InitParams(OpenGLInitParams {
                get_proc_address: get_proc,
                ctx: LoaderCtx { f: loader },
            }),
        ])?;

        let texture = NonZeroU32::new(texture.0.get())
            .ok_or_else(|| anyhow!("identifiant de texture nul"))?;

        Ok(Self {
            texture,
            fbo,
            render_ctx,
            _gl: gl,
        })
    }

    /// Appelé par mpv depuis son propre thread : uniquement un drapeau
    /// atomique. Aucune API Slint, aucune API mpv ici.
    pub fn set_update_callback(&mut self, callback: impl Fn() + Send + 'static) {
        self.render_ctx.set_update_callback(callback);
    }

    /// À appeler pendant `BeforeRendering`, sur le thread UI.
    pub fn render(&self) -> Result<()> {
        self.render_ctx.render::<LoaderCtx>(
            self.fbo.0.get() as i32,
            VIDEO_WIDTH as i32,
            VIDEO_HEIGHT as i32,
            // flip=false : mpv écrit avec l'origine en haut, convention
            // attendue par Slint (TopLeft). true afficherait à l'envers.
            false,
        )?;
        self.render_ctx.report_swap();
        Ok(())
    }
}
