use std::ffi::{c_void, CStr, CString};
use std::num::NonZeroU32;

use anyhow::{anyhow, Result};
use glow::HasContext;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};

pub const VIDEO_W: u32 = 1280;
pub const VIDEO_H: u32 = 720;

/// Contexte transmis à mpv pour résoudre les symboles GL.
pub struct LoaderCtx {
    f: &'static (dyn Fn(&CStr) -> *const c_void + Send + Sync),
}

/// Signature exigée par `OpenGLInitParams::get_proc_address` (pointeur de fonction).
fn get_proc(ctx: &LoaderCtx, name: &str) -> *mut c_void {
    let name = CString::new(name).expect("nom de symbole GL sans octet nul");
    (ctx.f)(name.as_c_str()) as *mut c_void
}

/// Décrit le backend graphique choisi par Slint (sonde de la Task 2).
pub fn describe_api(api: &slint::GraphicsAPI<'_>) -> Result<String> {
    match api {
        slint::GraphicsAPI::NativeOpenGL { get_proc_address } => {
            let gl = unsafe { glow::Context::from_loader_function_cstr(|s| get_proc_address(s)) };
            unsafe {
                Ok(format!(
                    "OpenGL · {} · {} · {}",
                    gl.get_parameter_string(glow::VENDOR),
                    gl.get_parameter_string(glow::RENDERER),
                    gl.get_parameter_string(glow::VERSION)
                ))
            }
        }
        _ => Err(anyhow!(
            "Slint n'utilise pas OpenGL : renderer incompatible"
        )),
    }
}

pub struct GlVideo {
    pub fbo: glow::Framebuffer,
    pub render_ctx: RenderContext<'static>,
    pub image: slint::Image,
    pub _texture: glow::Texture,
    pub _gl: glow::Context,
}

impl GlVideo {
    pub fn setup(api: &slint::GraphicsAPI<'_>, mpv: &'static libmpv2::Mpv) -> Result<Self> {
        let loader = match api {
            slint::GraphicsAPI::NativeOpenGL { get_proc_address } => *get_proc_address,
            _ => return Err(anyhow!("Slint n'utilise pas OpenGL")),
        };

        // SAFETY: le pointeur vient du renderer Slint et reste valide aussi
        // longtemps que la fenêtre, qui survit au RenderContext créé ci-dessous.
        let loader: &'static (dyn Fn(&CStr) -> *const c_void + Send + Sync) =
            unsafe { std::mem::transmute(loader) };

        let gl = unsafe { glow::Context::from_loader_function_cstr(|s| loader(s)) };

        let (texture, fbo) = unsafe {
            let texture = gl
                .create_texture()
                .map_err(|e| anyhow!("création de la texture GL: {e}"))?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                VIDEO_W as i32,
                VIDEO_H as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );

            let fbo = gl
                .create_framebuffer()
                .map_err(|e| anyhow!("création du FBO: {e}"))?;
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

        let non_zero = NonZeroU32::new(texture.0.get())
            .ok_or_else(|| anyhow!("identifiant de texture nul"))?;
        let image = unsafe {
            slint::BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                non_zero,
                (VIDEO_W, VIDEO_H).into(),
            )
            .build()
        };

        Ok(Self {
            fbo,
            render_ctx,
            image,
            _texture: texture,
            _gl: gl,
        })
    }
}
