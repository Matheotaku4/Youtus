# M0 — Spike vidéo Slint + libmpv : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prouver qu'une vidéo 1080p60 rendue par libmpv dans une texture OpenGL s'affiche et se contrôle dans une fenêtre Slint sous Wayland, à 60 fps, sans que le rendering notifier de FemtoVG (slint#12030) ne détruise les performances.

**Architecture:** Le rendering notifier de Slint fournit le loader OpenGL de son contexte ; on y crée une texture RGBA 1280×720 + un FBO, on y rend mpv frame par frame, et Slint affiche cette texture comme une `Image` ordinaire. mpv est un singleton `&'static` (Box::leak) pour obtenir un `RenderContext<'static>` stockable.

**Tech Stack:** Rust 2021, Slint 1.18 (winit + FemtoVG), libmpv2 7.x (API render OpenGL), glow 0.16, Wayland.

**Spec:** `docs/superpowers/specs/2026-11-19-youtus-design.md`

## Global Constraints

- Plateforme : Linux, session Wayland (X11/XWayland accepté en repli).
- Slint 1.18.x, backend winit, renderer FemtoVG (défaut).
- libmpv2 7.x, API render OpenGL uniquement — jamais `wid`.
- Texture vidéo : `GL_TEXTURE_2D`, RGBA8, **1280×720 fixe** (Slint fait le scaling).
- Taille de fenêtre du spike : 1280×800.
- Le spike est **jetable** : il vit dans `spikes/m0-slint-mpv/`, n'entre pas dans le workspace final, et son code n'est pas réutilisé tel quel.
- Aucun appel à l'API mpv depuis le callback de frame (`set_update_callback`) : uniquement un flag atomique + `request_redraw`.
- Aucun `unwrap()` sur une valeur renvoyée par mpv.
- Français pour les messages UI et les commentaires.

---

## Prérequis (à exécuter sur la machine de l'utilisateur)

- [ ] **Installer la toolchain Rust**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
source "$HOME/.cargo/env"
rustc --version   # attendu : rustc 1.8x ou plus
```

- [ ] **Installer les dépendances système (Debian/Ubuntu)**

```bash
sudo apt update
sudo apt install -y libmpv-dev libwayland-dev libxkbcommon-dev \
  libgl1-mesa-dev libegl1-mesa-dev libfontconfig1-dev libfreetype6-dev \
  pkg-config build-essential
pkg-config --modversion mpv   # attendu : une version (0.3x / 0.40)
```

- [ ] **Équivalent Fedora (si besoin)**

```bash
sudo dnf install -y mpv-libs-devel wayland-devel libxkbcommon-devel \
  mesa-libGL-devel mesa-libEGL-devel fontconfig-devel freetype-devel \
  pkgconf-pkg-config gcc gcc-c++
```

- [ ] **Générer une vidéo de test 1080p60**

```bash
ffmpeg -y -f lavfi -i testsrc2=size=1920x1080:rate=60 \
  -f lavfi -i sine=frequency=440 \
  -t 30 -c:v libx264 -preset veryfast -pix_fmt yuv420p -c:a aac /tmp/m0.mp4
ls -lh /tmp/m0.mp4
```

- [ ] **Vérifier la session**

```bash
echo "$XDG_SESSION_TYPE"   # attendu : wayland
```

---

## Structure de fichiers

| Fichier | Responsabilité |
|---|---|
| `spikes/m0-slint-mpv/Cargo.toml` | Crate jetable, dépendances épinglées |
| `spikes/m0-slint-mpv/build.rs` | Compilation du `.slint` |
| `spikes/m0-slint-mpv/ui/app.slint` | Fenêtre, image vidéo, contrôles, overlay de mesures |
| `spikes/m0-slint-mpv/src/main.rs` | Câblage : notifier, timer de mesures, callbacks |
| `spikes/m0-slint-mpv/src/gl.rs` | Loader GL, texture + FBO, render context mpv |
| `spikes/m0-slint-mpv/src/player.rs` | Singleton mpv, commandes, `sanitize_time` |
| `docs/superpowers/spikes/2026-11-19-m0-verdict.md` | Mesures et décision |

---

### Task 1: Scaffold du crate + fenêtre Slint vide

**Files:**
- Create: `spikes/m0-slint-mpv/Cargo.toml`
- Create: `spikes/m0-slint-mpv/build.rs`
- Create: `spikes/m0-slint-mpv/ui/app.slint`
- Create: `spikes/m0-slint-mpv/src/main.rs`

**Interfaces:**
- Consumes: rien.
- Produces: `AppWindow` (généré par Slint) avec les propriétés `frame: image`, `status: string`, `playing: bool`, `position: float`, `duration: float`, `fps: float`, `render-ms: float`, `progress: float` et les callbacks `toggle-play()`, `seek-to(float)`. Le crate `m0-slint-mpv` compile et ouvre une fenêtre 1280×800.

- [ ] **Step 1: Créer `Cargo.toml`**

```toml
[package]
name = "m0-slint-mpv"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
slint = "1.18"
libmpv2 = "7"
glow = "0.16"
anyhow = "1"

[build-dependencies]
slint-build = "1.18"

[profile.release]
debug = true
```

- [ ] **Step 2: Créer `build.rs`**

```rust
fn main() {
    slint_build::compile("ui/app.slint").expect("compilation du .slint");
}
```

- [ ] **Step 3: Créer `ui/app.slint` (version minimale)**

```slint
export component AppWindow inherits Window {
    title: "M0 — Slint + libmpv";
    width: 1280px;
    height: 800px;
    background: #0f0f0f;

    in property <image> frame;
    in property <string> status: "démarrage";
    in property <bool> playing: false;
    in property <float> position: 0;
    in property <float> duration: 0;
    in property <float> fps: 0;
    in property <float> render-ms: 0;
    in property <float> progress: 0;

    callback toggle-play();
    callback seek-to(float);

    VerticalLayout {
        Rectangle {
            background: #000000;
            clip: true;

            Image {
                width: 100%;
                height: 100%;
                source: root.frame;
                image-fit: contain;
            }

            Text {
                x: 12px;
                y: 12px;
                text: root.status;
                color: #ffffff;
                font-size: 13px;
            }
        }

        Rectangle {
            height: 64px;
            background: #0f0f0f;

            HorizontalLayout {
                padding: 12px;
                spacing: 12px;

                Text {
                    text: root.playing ? "Pause" : "Lecture";
                    color: #f1f1f1;
                    vertical-alignment: center;
                    width: 72px;
                }
            }
        }
    }
}
```

- [ ] **Step 4: Créer `src/main.rs` (version minimale)**

```rust
slint::include_modules!();

fn main() -> anyhow::Result<()> {
    let app = AppWindow::new()?;
    app.run()?;
    Ok(())
}
```

- [ ] **Step 5: Compiler**

Run: `cd spikes/m0-slint-mpv && cargo build`
Expected: `Finished dev profile`. Si `libmpv2-sys` échoue sur `pkg-config`, c'est que `libmpv-dev` n'est pas installé (§ Prérequis).

- [ ] **Step 6: Lancer**

Run: `cargo run`
Expected: une fenêtre noire 1280×800 avec « démarrage ». `echo $XDG_SESSION_TYPE` doit afficher `wayland` ; si Slint bascule silencieusement sur X11, relancer avec `SLINT_BACKEND=winit-femtovg` et noter la sortie.

- [ ] **Step 7: Commit**

```bash
git add spikes/m0-slint-mpv
git commit -m "spike(m0): scaffold Slint window for mpv render test"
```

---

### Task 2: Sonde du GraphicsAPI (OpenGL ou rien)

**Files:**
- Modify: `spikes/m0-slint-mpv/src/main.rs`
- Modify: `spikes/m0-slint-mpv/ui/app.slint` (affichage du statut, déjà prévu)

**Interfaces:**
- Consumes: `AppWindow` de la Task 1.
- Produces: une fonction `describe_api(api: &slint::GraphicsAPI<'_>) -> String` qui renvoie le nom du backend et, en OpenGL, `GL_VENDOR` / `GL_RENDERER` / `GL_VERSION`. Le statut affiché indique impérativement `OpenGL` — sinon le spike s'arrête là.

- [ ] **Step 1: Brancher le notifier**

```rust
slint::include_modules!();

use slint::GraphicsAPI;

fn describe_api(api: &GraphicsAPI<'_>) -> anyhow::Result<String> {
    match api {
        GraphicsAPI::NativeOpenGL { get_proc_address } => {
            let gl = unsafe {
                glow::Context::from_loader_function_cstr(|s| get_proc_address(s))
            };
            unsafe {
                let vendor = gl.get_parameter_string(glow::VENDOR);
                let renderer = gl.get_parameter_string(glow::RENDERER);
                let version = gl.get_parameter_string(glow::VERSION);
                Ok(format!("OpenGL · {vendor} · {renderer} · {version}"))
            }
        }
        _ => anyhow::bail!("Slint n'utilise pas OpenGL : renderer incompatible"),
    }
}

fn main() -> anyhow::Result<()> {
    let app = AppWindow::new()?;

    let weak = app.as_weak();
    app.window().set_rendering_notifier(move |state, api| {
        if let slint::RenderingState::RenderingSetup = state {
            let message = match describe_api(&api) {
                Ok(info) => {
                    eprintln!("{info}");
                    info
                }
                Err(e) => {
                    eprintln!("ERREUR: {e:#}");
                    format!("ERREUR: {e}")
                }
            };
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = weak.upgrade() {
                    app.set_status(message.into());
                }
            });
        }
    })?;

    app.run()?;
    Ok(())
}
```

- [ ] **Step 2: Lancer et lire le statut**

Run: `cargo run 2>&1 | tee /tmp/m0-task2.log`
Expected: `OpenGL · <vendor> · <renderer> · <version>`. Sur une machine saine : `Mesa`/`Intel`/`AMD`/`NVIDIA`. Si le statut affiche « renderer incompatible » ou si `GL_VERSION` manque, **stop** : il faut essayer `SLINT_BACKEND=winit-femtovg` puis documenter dans le verdict.

- [ ] **Step 3: Commit**

```bash
git add spikes/m0-slint-mpv
git commit -m "spike(m0): probe Slint OpenGL backend and log GL strings"
```

---

### Task 3: mpv rendu dans un FBO, affiché comme `Image` Slint

**Files:**
- Create: `spikes/m0-slint-mpv/src/player.rs`
- Create: `spikes/m0-slint-mpv/src/gl.rs`
- Modify: `spikes/m0-slint-mpv/src/main.rs`
- Modify: `spikes/m0-slint-mpv/ui/app.slint` (rien à changer : `frame` existe déjà)

**Interfaces:**
- Consumes: `AppWindow::set_frame(image)`, `describe_api` (Task 2).
- Produces:
  - `player::MpvPlayer::new() -> anyhow::Result<MpvPlayer>` ;
  - `MpvPlayer::mpv(&self) -> &'static libmpv2::Mpv` ;
  - `MpvPlayer::load(&self, path: &str) -> anyhow::Result<()>` ;
  - `MpvPlayer::sanitize_time(position: f64, duration: f64) -> (f64, f64)` (testé) ;
  - `gl::GlVideo::setup(api: &slint::GraphicsAPI<'_>, mpv: &'static libmpv2::Mpv) -> anyhow::Result<GlVideo>` avec les champs `fbo: glow::Framebuffer`, `render_ctx: libmpv2::render::RenderContext<'static>`, `image: slint::Image`.

- [ ] **Step 1: Écrire les tests de `sanitize_time` (échec attendu)**

Dans `src/player.rs` :

```rust
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
```

- [ ] **Step 2: Lancer les tests, vérifier l'échec**

Run: `cargo test sanitize -- --nocapture`
Expected: FAIL, `cannot find function sanitize_time`.

- [ ] **Step 3: Implémenter `player.rs`**

```rust
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
        if d > 0.0 { position.min(d) } else { position }
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
```

- [ ] **Step 4: Lancer les tests, vérifier le succès**

Run: `cargo test sanitize`
Expected: `4 passed`.

- [ ] **Step 5: Implémenter `gl.rs`**

```rust
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

pub struct GlVideo {
    pub fbo: glow::Framebuffer,
    pub render_ctx: RenderContext<'static>,
    pub image: slint::Image,
    pub _texture: glow::Texture,
    pub _gl: glow::Context,
}

impl GlVideo {
    pub fn setup(
        api: &slint::GraphicsAPI<'_>,
        mpv: &'static libmpv2::Mpv,
    ) -> Result<Self> {
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
            let texture = gl.create_texture()?;
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

            let fbo = gl.create_framebuffer()?;
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
```

- [ ] **Step 6: Câbler dans `main.rs`**

```rust
mod gl;
mod player;

slint::include_modules!();

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/m0.mp4".to_string());

    let app = AppWindow::new()?;
    let player = Rc::new(player::MpvPlayer::new()?);
    let video: Rc<RefCell<Option<gl::GlVideo>>> = Rc::new(RefCell::new(None));
    let frame_pending = Arc::new(AtomicBool::new(true));

    {
        let player = player.clone();
        let video = video.clone();
        let frame_pending = frame_pending.clone();
        let weak = app.as_weak();
        let path = path.clone();

        app.window().set_rendering_notifier(move |state, api| match state {
            slint::RenderingState::RenderingSetup => {
                match gl::GlVideo::setup(&api, player.mpv()) {
                    Ok(mut v) => {
                        let pending = frame_pending.clone();
                        let weak = weak.clone();
                        v.render_ctx.set_update_callback(move || {
                            pending.store(true, Ordering::Release);
                            let weak = weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = weak.upgrade() {
                                    app.window().request_redraw();
                                }
                            });
                        });

                        let image = v.image.clone();
                        *video.borrow_mut() = Some(v);

                        let weak_ui = weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = weak_ui.upgrade() {
                                app.set_frame(image);
                                app.set_status("lecture".into());
                            }
                        });

                        if let Err(e) = player.load(&path) {
                            eprintln!("chargement: {e:#}");
                        }
                    }
                    Err(e) => {
                        eprintln!("setup GL: {e:#}");
                        let weak_ui = weak.clone();
                        let message = format!("ERREUR GL: {e}");
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = weak_ui.upgrade() {
                                app.set_status(message.into());
                            }
                        });
                    }
                }
            }
            slint::RenderingState::BeforeRendering => {
                if let Some(v) = video.borrow().as_ref() {
                    if frame_pending.swap(false, Ordering::AcqRel) {
                        if let Err(e) = v.render_ctx.render::<gl::LoaderCtx>(
                            v.fbo.0.get() as i32,
                            gl::VIDEO_W as i32,
                            gl::VIDEO_H as i32,
                            true,
                        ) {
                            eprintln!("render: {e}");
                        }
                        v.render_ctx.report_swap();
                    }
                }
            }
            slint::RenderingState::RenderingTeardown => {
                *video.borrow_mut() = None;
            }
            _ => {}
        })?;
    }

    {
        let player = player.clone();
        let weak = app.as_weak();
        app.on_toggle_play(move || {
            let _ = player.set_paused(!player.paused());
            if let Some(app) = weak.upgrade() {
                app.set_playing(!player.paused());
            }
        });
    }

    app.run()?;
    Ok(())
}
```

- [ ] **Step 7: Lancer avec la vidéo de test**

Run: `cargo run --release -- /tmp/m0.mp4`
Expected: la vidéo 1080p60 s'affiche dans la zone noire, avec le texte « lecture ». Si l'image est à l'envers, inverser le `true` en `false` dans `render(...)` (paramètre `flip`) et relancer. Si la fenêtre reste noire alors que `time-pos` avance, le problème est dans le FBO/texture, pas dans mpv.

- [ ] **Step 8: Commit**

```bash
git add spikes/m0-slint-mpv
git commit -m "spike(m0): render libmpv into an FBO and display it as a Slint image"
```

---

### Task 4: Contrôles + mesures (#12030)

**Files:**
- Modify: `spikes/m0-slint-mpv/ui/app.slint`
- Modify: `spikes/m0-slint-mpv/src/main.rs`

**Interfaces:**
- Consumes: `MpvPlayer::{position, duration, set_paused, seek_absolute, paused}`, `GlVideo::render_ctx`.
- Produces: propriétés UI `fps`, `render-ms`, `progress`, `position`, `duration`, `playing` mises à jour chaque seconde ; callbacks `toggle-play()` et `seek-to(fraction)` fonctionnels.

- [ ] **Step 1: Remplacer le pied de page par les vrais contrôles**

Dans `ui/app.slint`, remplacer le `Rectangle { height: 64px; ... }` par :

```slint
        Rectangle {
            height: 72px;
            background: #0f0f0f;

            HorizontalLayout {
                padding: 12px;
                spacing: 12px;

                Rectangle {
                    width: 88px;
                    height: 40px;
                    border-radius: 8px;
                    background: #272727;

                    Text {
                        text: root.playing ? "Pause" : "Lecture";
                        color: #f1f1f1;
                        horizontal-alignment: center;
                        vertical-alignment: center;
                        font-size: 14px;
                    }

                    touch := TouchArea {
                        clicked => { root.toggle-play(); }
                    }
                }

                Text {
                    text: root.progress-label;
                    color: #f1f1f1;
                    vertical-alignment: center;
                    width: 110px;
                    font-size: 13px;
                }

                Rectangle {
                    Rectangle {
                        y: (parent.height - 4px) / 2;
                        height: 4px;
                        width: parent.width;
                        border-radius: 2px;
                        background: #3f3f3f;
                    }

                    Rectangle {
                        y: (parent.height - 4px) / 2;
                        height: 4px;
                        width: parent.width * root.progress;
                        border-radius: 2px;
                        background: #f10000;
                    }

                    bar := TouchArea {
                        clicked => {
                            if (bar.width > 0) {
                                root.seek-to(bar.mouse-x / bar.width);
                            }
                        }
                    }
                }

                Text {
                    text: root.metrics;
                    color: #aaaaaa;
                    vertical-alignment: center;
                    width: 190px;
                    font-size: 12px;
                }
            }
        }
```

Ajouter les propriétés et le helper de formatage en haut de `AppWindow` :

```slint
    in property <string> progress-label: "0:00 / 0:00";
    in property <string> metrics: "";
```

Et à la fin de `main.rs`, dans le timer, composer ces deux libellés en Rust (plus simple que des maths en Slint) :

```rust
app.set_progress_label(
    format!("{}:{:02} / {}:{:02}", (p / 60.0) as u32, (p % 60.0) as u32,
            (d / 60.0) as u32, (d % 60.0) as u32).into(),
);
app.set_metrics(format!("{fps} fps · {ms:.1} ms/frame").into());
```

- [ ] **Step 2: Ajouter les compteurs et le timer dans `main.rs`**

En haut de `main` :

```rust
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

let frames = Arc::new(AtomicU64::new(0));
let render_us = Arc::new(AtomicU64::new(0));
```

Ajouter aussi `let frames = frames.clone();` et `let render_us = render_us.clone();` dans le bloc de captures du notifier (juste avant `let weak = app.as_weak();` du bloc `{ let player = player.clone(); ... }`), sinon le compteur reste à zéro.

Dans `BeforeRendering`, autour du rendu :

```rust
let started = Instant::now();
if let Err(e) = v.render_ctx.render::<gl::LoaderCtx>(
    v.fbo.0.get() as i32,
    gl::VIDEO_W as i32,
    gl::VIDEO_H as i32,
    true,
) {
    eprintln!("render: {e}");
}
v.render_ctx.report_swap();
frames.fetch_add(1, Ordering::Relaxed);
render_us.fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
```

Après le bloc du notifier :

```rust
let timer = slint::Timer::default();
{
    let player = player.clone();
    let frames = frames.clone();
    let render_us = render_us.clone();
    let weak = app.as_weak();

    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(1),
        move || {
            let count = frames.swap(0, Ordering::Relaxed).max(1);
            let us = render_us.swap(0, Ordering::Relaxed);
            let fps = count as f32;
            let ms = us as f32 / count as f32 / 1000.0;

            let (p, d) = player::sanitize_time(
                player.time_position(),
                player.duration(),
            );
            let progress = if d > 0.0 { (p / d) as f32 } else { 0.0 };

            if let Some(app) = weak.upgrade() {
                app.set_fps(fps);
                app.set_render_ms(ms);
                app.set_playing(!player.paused());
                app.set_position(p as f32);
                app.set_duration(d as f32);
                app.set_progress(progress);
                app.set_progress_label(
                    format!(
                        "{}:{:02} / {}:{:02}",
                        (p / 60.0) as u32,
                        (p % 60.0) as u32,
                        (d / 60.0) as u32,
                        (d % 60.0) as u32
                    )
                    .into(),
                );
                app.set_metrics(format!("{fps:.0} fps · {ms:.1} ms/frame").into());
            }
        },
    );
}
let _timer = timer; // garde le timer vivant jusqu'à la fin de main
```

- [ ] **Step 3: Ajouter le callback de seek**

```rust
{
    let player = player.clone();
    app.on_seek_to(move |fraction| {
        let (_, d) = player::sanitize_time(player.time_position(), player.duration());
        if d > 0.0 {
            let _ = player.seek_absolute((fraction as f64 * d).clamp(0.0, d));
        }
    });
}
```

- [ ] **Step 4: Lancer et vérifier les contrôles**

Run: `cargo run --release -- /tmp/m0.mp4`
Expected: lecture auto, bouton Pause fonctionnel, clic sur la barre = seek, compteur `60 fps · x.x ms/frame` qui bouge.

- [ ] **Step 5: Protocole de mesure (à reporter dans le verdict)**

Pour chaque essai, noter dans un tableau : session (`echo $XDG_SESSION_TYPE`), compositeur, GPU (`glxinfo -B | head -20` ou `nvidia-smi --query-gpu=name --format=csv,noheader`), fps affiché (min/moyen sur 30 s), ms/frame moyen, CPU (`top -b -n 1 -p $(pgrep -f m0-slint-mpv | head -1) | tail -1`), RSS (`ps -o rss= -p $(pgrep -f m0-slint-mpv | head -1)`).

Essais obligatoires :
1. fenêtre 1280×800, lecture 1080p60 ;
2. fenêtre redimensionnée à 1920×1080, lecture 1080p60 ;
3. pause pendant 10 s (le compteur doit rester à 0 fps, CPU quasi nul) ;
4. seek répété 10 fois pendant la lecture (pas de gel > 500 ms).

- [ ] **Step 6: Commit**

```bash
git add spikes/m0-slint-mpv
git commit -m "spike(m0): player controls and frame-time instrumentation"
```

---

### Task 5: Verdict écrit et décision

**Files:**
- Create: `docs/superpowers/spikes/2026-11-19-m0-verdict.md`

**Interfaces:**
- Consumes: les mesures de la Task 4.
- Produces: une décision explicite `GO FemtoVG`, `GO Skia-OpenGL` ou `NO-GO (passer au rendu logiciel)` qui conditionne le plan M1.

- [ ] **Step 1: Écrire le verdict**

```markdown
# Verdict M0 — Slint + libmpv

## Environnement
- Distro :
- Compositeur : (GNOME Shell / KWin / Sway / Hyprland)
- Session : wayland
- GPU / pilote :
- Version Slint / libmpv / yt-dlp :

## Mesures
| Essai | fps min | fps moyen | ms/frame | CPU % | RSS Mo |
|---|---|---|---|---|---|
| 1280×800 | | | | | |
| 1920×1080 | | | | | |
| pause 10 s | | | | | |
| 10 seeks | | | | | |

## Verdict
- [ ] GO FemtoVG — 60 fps tenus, ms/frame < 4, CPU < 20 %
- [ ] GO Skia-OpenGL — FemtoVG insuffisant, refaire les Tasks 2–4 avec `SLINT_BACKEND=winit-skia-opengl`
- [ ] NO-GO — envisager `MPV_RENDER_API_TYPE_SW`, ou réévaluer Tauri/GTK

## Décisions dérivées
- Taille de texture fixe suffisante ? (oui/non)
- Besoin de déplacer les commandes mpv sur un client dédié (`Mpv::create_client`) ?
- Image à l'envers corrigée par `flip` ou par `BorrowedOpenGLTextureOrigin` ?
```

- [ ] **Step 2: Renseigner les mesures et cocher la décision**

Run: `cargo run --release -- /tmp/m0.mp4` (une dernière fois, 30 s, chiffres stables)
Expected: le tableau est rempli avec des valeurs réelles, pas des estimations.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/spikes/2026-11-19-m0-verdict.md
git commit -m "docs(m0): spike verdict and renderer decision"
```

---

## Vérification finale du spike

- [ ] `cd spikes/m0-slint-mpv && cargo test` → 4 tests passent.
- [ ] `cargo run --release -- /tmp/m0.mp4` → vidéo visible, contrôles fonctionnels, compteur de fps cohérent.
- [ ] `docs/superpowers/spikes/2026-11-19-m0-verdict.md` contient des mesures réelles et une décision cochée.
- [ ] Aucune occurrence de `wid` dans le code du spike (`grep -r wid spikes/`).

## Après M0

Le plan suivant (M1) ne sera écrit qu'après le verdict, car il dépend du renderer retenu :

- **M1** : workspace `youtus-*`, shell Slint, OAuth device flow, `browse` accueil/abonnements, miniatures, cache SQLite.
- **S1 (parallèle, sans GUI)** : sonde de capacités OAuth (accueil/historique/like/`playbackTracking`/`createCommentParams`) pour figer la matrice de capacités et débloquer M3/M4.
- **M2** : page Watch, yt-dlp, intégration du lecteur validé en M0.
