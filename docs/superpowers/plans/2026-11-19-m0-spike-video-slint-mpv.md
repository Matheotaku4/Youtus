# M0 — Spike vidéo Slint + libmpv : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prouver qu'une vidéo 1080p60 rendue par libmpv dans une texture OpenGL s'affiche et se contrôle dans une fenêtre Slint sous Wayland, à 60 fps, sans que le rendering notifier de FemtoVG (slint#12030) ne détruise les performances.

**Architecture:** Le rendering notifier de Slint fournit le loader OpenGL de son contexte ; on y crée une texture RGBA 1280×720 + un FBO, on y rend mpv frame par frame, et Slint affiche cette texture comme une `Image` ordinaire. mpv est un singleton `&'static` (Box::leak) pour obtenir un `RenderContext<'static>` stockable. Aucune donnée Slint ne traverse de thread : le callback de frame de mpv ne pose qu'un drapeau atomique, un timer UI le consomme.

**Tech Stack:** Rust 2021, Slint 1.19 (winit + FemtoVG), libmpv2 7.0.1 (API render OpenGL), glow 0.16, Wayland.

**Spec:** `docs/superpowers/specs/2026-11-19-youtus-design.md`

## État d'exécution (2026-11-19)

| Tâche | État | Preuve |
|---|---|---|
| 1 — scaffold | **fait** | `cargo check` OK |
| 2 — sonde GraphicsAPI | **fait** (code) | compile ; exécution à faire sur Wayland |
| 3 — mpv → FBO → texture | **fait** (code) | compile ; exécution à faire |
| 4 — contrôles + mesures | **fait** (code) | compile ; mesures à faire |
| 5 — verdict | à faire | nécessite l'exécution réelle |

`cargo check` a été exécuté dans un sandbox **sans root, sans serveur graphique** : il valide les API et les types, **pas** l'édition de liens, ni le rendu, ni les performances. Voir « Journal de vérification » en fin de document.

## Global Constraints

- Plateforme : Linux, session Wayland (X11/XWayland accepté en repli).
- Slint 1.19.x, backend winit, renderer FemtoVG (défaut). `slint = "1.18"` résout aujourd'hui 1.19.0.
- libmpv2 7.x, API render OpenGL uniquement — jamais `wid`.
- Texture vidéo : `GL_TEXTURE_2D`, RGBA8, **1280×720 fixe** (Slint fait le scaling).
- Taille de fenêtre du spike : 1280×800.
- Le spike est **jetable** : il vit dans `spikes/m0-slint-mpv/`, n'entre pas dans le workspace final, et son code n'est pas réutilisé tel quel.
- **Aucune API mpv ni Slint dans le callback de frame** (`set_update_callback`) : uniquement un flag atomique.
- **`slint::Image` et `slint::Weak` ne sont pas `Send`** : jamais dans `invoke_from_event_loop` ni dans un canal. Le transfert notifier → UI se fait par un `Rc<RefCell<Option<UiUpdate>>>` lu sur le thread UI.
- `glow::create_texture()` / `create_framebuffer()` renvoient `Result<_, String>` : convertir explicitement avec `map_err` (pas de `?` direct dans `anyhow`).
- Aucun `unwrap()` sur une valeur renvoyée par mpv.
- Français pour les messages UI et les commentaires.

---

## Prérequis (machine de l'utilisateur, avec root)

- [ ] **Toolchain Rust**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
source "$HOME/.cargo/env"
rustc --version
```

- [ ] **Dépendances système (Debian/Ubuntu)**

```bash
sudo apt update
sudo apt install -y libmpv-dev libwayland-dev libxkbcommon-dev \
  libgl1-mesa-dev libegl1-mesa-dev libfontconfig1-dev libfreetype6-dev \
  pkg-config build-essential
pkg-config --modversion mpv
```

- [ ] **Équivalent Fedora**

```bash
sudo dnf install -y mpv-libs-devel wayland-devel libxkbcommon-devel \
  mesa-libGL-devel mesa-libEGL-devel fontconfig-devel freetype-devel \
  pkgconf-pkg-config gcc gcc-c++
```

- [ ] **Vidéo de test 1080p60**

```bash
ffmpeg -y -f lavfi -i testsrc2=size=1920x1080:rate=60 \
  -f lavfi -i sine=frequency=440 \
  -t 30 -c:v libx264 -preset veryfast -pix_fmt yuv420p -c:a aac /tmp/m0.mp4
```

- [ ] **Session**

```bash
echo "$XDG_SESSION_TYPE"   # attendu : wayland
```

### Variante sans root (sandbox de vérification)

Utilisée pour obtenir le `cargo check` de ce plan. Ne remplace pas les paquets système sur une vraie machine, et **ne permet pas** de lancer l'application (pas de serveur graphique).

```bash
mkdir -p vendor/debs vendor/libmpv vendor/pkgconfig && cd vendor/debs
BASE=https://archive.ubuntu.com/ubuntu/pool/universe/m/mpv
curl -sSLO $BASE/libmpv-dev_0.37.0-1ubuntu4_amd64.deb
curl -sSLO $BASE/libmpv2_0.37.0-1ubuntu4_amd64.deb
dpkg -x libmpv-dev_0.37.0-1ubuntu4_amd64.deb ../libmpv
dpkg -x libmpv2_0.37.0-1ubuntu4_amd64.deb ../libmpv

cat > ../pkgconfig/fontconfig.pc <<'EOF'
prefix=/usr
libdir=${prefix}/lib/x86_64-linux-gnu
includedir=${prefix}/include
Name: fontconfig
Description: stub local pour cargo check sans root
Version: 2.14.2
Libs: -lfontconfig
Cflags:
EOF

export LIBRARY_PATH=$PWD/../../vendor/libmpv/usr/lib/x86_64-linux-gnu
export PKG_CONFIG_PATH=$PWD/../../vendor/pkgconfig
cd ../../spikes/m0-slint-mpv && cargo check
```

Notes : `libmpv2-sys` embarque des bindings pré-générés et n'appelle pas pkg-config (pas besoin de clang) ; `fontique` exige les symboles racine de `fontconfig-sys`, donc `RUST_FONTCONFIG_DLOPEN` **ne marche pas** — d'où le `.pc` stub, suffisant car `cargo check` ne linke pas.

---

## Structure de fichiers

| Fichier | Responsabilité |
|---|---|
| `spikes/m0-slint-mpv/Cargo.toml` | Crate jetable, dépendances épinglées |
| `spikes/m0-slint-mpv/build.rs` | Compilation du `.slint` |
| `spikes/m0-slint-mpv/ui/app.slint` | Fenêtre, image vidéo, contrôles, overlay de mesures |
| `spikes/m0-slint-mpv/src/main.rs` | Câblage : notifier, timers, callbacks |
| `spikes/m0-slint-mpv/src/gl.rs` | Loader GL, texture + FBO, render context mpv |
| `spikes/m0-slint-mpv/src/player.rs` | Singleton mpv, commandes, réexport de `sanitize_time` |
| `spikes/m0-core/src/lib.rs` | `sanitize_time` + tests, **zéro dépendance** (testable sans libmpv) |
| `scripts/m0-measure.sh` | Mesures automatisées (fps, ms/frame, CPU, RSS) + remplissage du verdict |
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
- Produces: `AppWindow` avec `frame: image`, `status: string`, `playing: bool`, `position: float`, `duration: float`, `fps: float`, `render-ms: float`, `progress: float`, `progress-label: string`, `metrics: string` et les callbacks `toggle-play()`, `seek-to(float)`.

- [x] **Step 1: `Cargo.toml`** (fait)

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

- [x] **Step 2: `build.rs`** (fait)

```rust
fn main() {
    slint_build::compile("ui/app.slint").expect("compilation du .slint");
}
```

- [x] **Step 3: `ui/app.slint`** (fait — version finale, avec les contrôles de la Task 4)

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
    in property <string> progress-label: "0:00 / 0:00";
    in property <string> metrics: "";

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
    }
}
```

- [x] **Step 4: `src/main.rs` minimal** — remplacé par la Task 3.
- [x] **Step 5: `cargo check`** → `Finished dev profile`, zéro warning.

---

### Task 2: Sonde du GraphicsAPI (OpenGL ou rien)

**Files:**
- Modify: `spikes/m0-slint-mpv/src/main.rs`

**Interfaces:**
- Consumes: `AppWindow`.
- Produces: `describe_api(api: &slint::GraphicsAPI<'_>) -> anyhow::Result<String>` renvoyant `OpenGL · <vendor> · <renderer> · <version>`. Si le backend n'est pas OpenGL, le spike s'arrête là.

- [x] **Step 1: Code de la sonde** (fait — `gl::describe_api`, compilé, appelé au démarrage)

```rust
/// Décrit le backend graphique choisi par Slint.
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
        _ => Err(anyhow!("Slint n'utilise pas OpenGL : renderer incompatible")),
    }
}
```

Appel dans le notifier : `if let Ok(info) = gl::describe_api(&api) { eprintln!("{info}"); }`. Le résultat part sur stderr, où il est récupéré par `tee` dans l'étape suivante.

- [ ] **Step 2: Lancer et lire le statut** (sur Wayland)

Run: `cargo run 2>&1 | tee /tmp/m0-task2.log`
Expected: `OpenGL · <vendor> · <renderer> · <version>`.

---

### Task 3: mpv rendu dans un FBO, affiché comme `Image` Slint

**Files:**
- Create: `spikes/m0-slint-mpv/src/player.rs`
- Create: `spikes/m0-slint-mpv/src/gl.rs`
- Modify: `spikes/m0-slint-mpv/src/main.rs`

**Interfaces:**
- Consumes: `describe_api` (Task 2).
- Produces:
  - `player::sanitize_time(position: f64, duration: f64) -> (f64, f64)` (testé) ;
  - `player::MpvPlayer::new()`, `mpv()`, `load(&str)`, `set_paused`, `paused`, `time_position`, `duration`, `seek_absolute` ;
  - `gl::GlVideo::setup(api, mpv) -> Result<GlVideo>` avec `fbo`, `render_ctx: RenderContext<'static>`, `image: slint::Image` ;
  - `enum UiUpdate { Frame(Image, String), Status(String) }`.

- [x] **Step 1: Tests de `sanitize_time`** (fait — exécutés, 4/4)

Dans `spikes/m0-core/src/lib.rs` : crate `m0-core`, **aucune dépendance** (ni libmpv, ni Slint, ni système graphique), donc exécutable partout.

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

Vérification : `cargo test --manifest-path spikes/m0-core/Cargo.toml` → `4 passed; 0 failed` (journal de vérification en fin de document).

- [x] **Step 2: `player.rs`** — voir le fichier du dépôt ; il réexporte la logique pure (`pub use m0_core::sanitize_time;`). Points d'API vérifiés :

```rust
let mpv = libmpv2::Mpv::with_initializer(|init| {
    for (key, value) in [("vo", "libmpv"), ("hwdec", "auto-safe"), ("osc", "no"), /* … */] {
        init.set_option(key, value)?;
    }
    Ok(())
})?;
// handle 'static : nécessaire pour obtenir un RenderContext<'static>
Ok(Self { mpv: Box::leak(Box::new(mpv)) })
```

`MpvPlayer` expose `time_position()`/`duration()` en lisant `time-pos`/`duration` avec `get_property::<f64>().unwrap_or(f64::NAN)` — jamais d'`unwrap()`.

- [x] **Step 3: `gl.rs`** — voir le fichier du dépôt. Deux pièges corrigés après compilation :

```rust
// glow renvoie Result<_, String> : `?` ne convertit pas vers anyhow
let texture = gl
    .create_texture()
    .map_err(|e| anyhow!("création de la texture GL: {e}"))?;
let fbo = gl
    .create_framebuffer()
    .map_err(|e| anyhow!("création du FBO: {e}"))?;
```

```rust
// loader Slint -> pointeur de fonction attendu par libmpv
fn get_proc(ctx: &LoaderCtx, name: &str) -> *mut c_void { … }

let render_ctx = mpv.create_render_context(vec![
    RenderParam::ApiType(RenderParamApiType::OpenGl),
    RenderParam::InitParams(OpenGLInitParams {
        get_proc_address: get_proc,
        ctx: LoaderCtx { f: loader },
    }),
])?;

// render() a un générique non inférable : turbofish obligatoire
v.render_ctx.render::<gl::LoaderCtx>(fbo_id, 1280, 720, true)?;
```

- [x] **Step 4: `main.rs`** (fait) — squelette exact, sans instrumentation :

```rust
mod gl;
mod player;

slint::include_modules!();

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// `slint::Image` et `slint::Weak` ne sont pas `Send` : la mise à jour d'UI
/// reste sur le thread UI, dans ce slot lu par le timer de redraw.
enum UiUpdate {
    Frame(slint::Image, String),
    Status(String),
}

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "/tmp/m0.mp4".to_string());

    let app = AppWindow::new()?;
    let player = Rc::new(player::MpvPlayer::new()?);
    let video: Rc<RefCell<Option<gl::GlVideo>>> = Rc::new(RefCell::new(None));
    let frame_pending = Arc::new(AtomicBool::new(true));
    let pending_ui: Rc<RefCell<Option<UiUpdate>>> = Rc::new(RefCell::new(None));

    {
        let player = player.clone();
        let video = video.clone();
        let frame_pending = frame_pending.clone();
        let pending_ui = pending_ui.clone();
        let path = path.clone();

        app.window().set_rendering_notifier(move |state, api| match state {
            slint::RenderingState::RenderingSetup => {
                match gl::GlVideo::setup(&api, player.mpv()) {
                    Ok(mut v) => {
                        let pending = frame_pending.clone();
                        v.render_ctx.set_update_callback(move || {
                            pending.store(true, Ordering::Release);
                        });
                        *pending_ui.borrow_mut() =
                            Some(UiUpdate::Frame(v.image.clone(), "lecture".into()));
                        *video.borrow_mut() = Some(v);
                        if let Err(e) = player.load(&path) {
                            eprintln!("chargement: {e:#}");
                        }
                    }
                    Err(e) => {
                        eprintln!("setup GL: {e:#}");
                        *pending_ui.borrow_mut() = Some(UiUpdate::Status(format!("ERREUR GL: {e}")));
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

    let redraw = slint::Timer::default();
    {
        let frame_pending = frame_pending.clone();
        let pending_ui = pending_ui.clone();
        let weak = app.as_weak();
        redraw.start(slint::TimerMode::Repeated, Duration::from_millis(8), move || {
            let Some(app) = weak.upgrade() else { return };
            if let Some(update) = pending_ui.borrow_mut().take() {
                match update {
                    UiUpdate::Frame(image, status) => {
                        app.set_frame(image);
                        app.set_status(status.into());
                    }
                    UiUpdate::Status(status) => app.set_status(status.into()),
                }
            }
            if frame_pending.load(Ordering::Acquire) {
                app.window().request_redraw();
            }
        });
    }
    let _redraw = redraw;

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

- [ ] **Step 5: Lancer avec la vidéo de test** (sur Wayland)

Run: `cargo run --release -- /tmp/m0.mp4`
Expected: la vidéo s'affiche dans la zone noire, texte « lecture ». Image à l'envers → passer le `flip` de `render(...)` à `false`. Fenêtre noire alors que `time-pos` avance → problème FBO/texture.

---

### Task 4: Contrôles + mesures (#12030)

**Files:**
- Modify: `spikes/m0-slint-mpv/src/main.rs`

**Interfaces:**
- Consumes: `MpvPlayer`, `GlVideo::render_ctx`, `UiUpdate`.
- Produces: propriétés `fps`, `render-ms`, `progress`, `progress-label`, `metrics` mises à jour chaque seconde ; callback `seek-to(fraction)`.

- [x] **Step 1: UI** — déjà dans `ui/app.slint` (Task 1, Step 3 : bouton, barre cliquable, libellés).

- [x] **Step 2: Instrumenter `main.rs`** (fait) — trois ajouts, le reste du fichier de la Task 3 est inchangé :

```rust
// 1. compteurs
let frames = Arc::new(AtomicU64::new(0));
let render_us = Arc::new(AtomicU64::new(0));

// 2. autour du rendu, dans BeforeRendering
let started = Instant::now();
if let Err(e) = v.render_ctx.render::<gl::LoaderCtx>(/* … */) {
    eprintln!("render: {e}");
}
v.render_ctx.report_swap();
frames.fetch_add(1, Ordering::Relaxed);
render_us.fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);

// 3. timer de statistiques (1 s), à côté du timer de redraw
let timer = slint::Timer::default();
{
    let player = player.clone();
    let frames = frames.clone();
    let render_us = render_us.clone();
    let weak = app.as_weak();
    timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
        let count = frames.swap(0, Ordering::Relaxed);
        let us = render_us.swap(0, Ordering::Relaxed);
        let fps = count as f32;
        let ms = if count > 0 { us as f32 / count as f32 / 1000.0 } else { 0.0 };
        let (p, d) = player::sanitize_time(player.time_position(), player.duration());
        let progress = if d > 0.0 { (p / d) as f32 } else { 0.0 };
        if let Some(app) = weak.upgrade() {
            app.set_fps(fps);
            app.set_render_ms(ms);
            app.set_playing(!player.paused());
            app.set_position(p as f32);
            app.set_duration(d as f32);
            app.set_progress(progress);
            app.set_progress_label(format!(
                "{}:{:02} / {}:{:02}",
                (p / 60.0) as u32, (p % 60.0) as u32,
                (d / 60.0) as u32, (d % 60.0) as u32
            ).into());
            app.set_metrics(format!("{fps:.0} fps · {ms:.1} ms/frame").into());
        }
    });
}
let _timer = timer;
```

Les clones `frames`/`render_us`/`pending_ui` doivent être ajoutés dans le bloc de captures du notifier.

**4. Métriques pour le script** (déjà dans `main.rs`) : si `M0_METRICS` est défini, l'application écrit un CSV `t_s,fps,render_ms,position_s,duration_s,playing` à chaque tick d'une seconde ; `M0_AUTOPAUSE_SEC=<n>` met la lecture en pause à n secondes, ce qui permet de mesurer l'inactivité sans interaction souris.

```rust
let mut metrics = match std::env::var("M0_METRICS") {
    Ok(path) => {
        let mut file = File::create(&path)?;
        writeln!(file, "t_s,fps,render_ms,position_s,duration_s,playing")?;
        Some(file)
    }
    Err(_) => None,
};
let autopause = std::env::var("M0_AUTOPAUSE_SEC").ok().and_then(|v| v.parse::<f64>().ok());

// dans le timer d'une seconde :
if let Some(file) = metrics.as_mut() {
    writeln!(file, "{:.0},{:.0},{:.2},{:.2},{:.2},{}",
        started_at.elapsed().as_secs_f64(), fps, ms, p, d, u8::from(!player.paused()))?;
}
```

- [x] **Step 3: Callback de seek** (fait)

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

- [ ] **Step 4: Vérifier les contrôles** (sur Wayland)

Run: `cargo run --release -- /tmp/m0.mp4`
Expected: lecture auto, bouton Pause, clic sur la barre = seek, compteur `60 fps · x.x ms/frame`.

- [ ] **Step 5: Protocole de mesure** (à reporter dans le verdict)

Pour chaque essai : session, compositeur, GPU, fps min/moyen sur 30 s, ms/frame, CPU (`top -b -n 1 -p $(pgrep -f m0-slint-mpv | head -1)`), RSS (`ps -o rss= -p …`).

Essais obligatoires : 1280×800 ; redimensionné 1920×1080 ; pause 10 s ; 10 seeks consécutifs.

---

### Task 5: Verdict écrit et décision

**Files:**
- Create: `docs/superpowers/spikes/2026-11-19-m0-verdict.md`

**Interfaces:**
- Consumes: mesures de la Task 4.
- Produces: décision `GO FemtoVG` / `GO Skia-OpenGL` / `NO-GO (rendu logiciel)` qui conditionne le plan M1.

- [ ] **Step 1: Écrire le verdict**

```markdown
# Verdict M0 — Slint + libmpv

## Environnement
- Distro :
- Compositeur :
- Session : wayland
- GPU / pilote :
- Versions : slint / libmpv / yt-dlp

## Mesures
| Essai | fps min | fps moyen | ms/frame | CPU % | RSS Mo |
|---|---|---|---|---|---|
| 1280×800 | | | | | |
| 1920×1080 | | | | | |
| pause 10 s | | | | | |
| 10 seeks | | | | | |

## Verdict
- [ ] GO FemtoVG — 60 fps tenus, ms/frame < 4, CPU < 20 %
- [ ] GO Skia-OpenGL — `SLINT_BACKEND=winit-skia-opengl`, refaire les Tasks 3–4
- [ ] NO-GO — `MPV_RENDER_API_TYPE_SW`, ou réévaluer le choix d'UI

## Décisions dérivées
- Texture fixe suffisante ?
- Commandes mpv à déplacer sur un client dédié (`Mpv::create_client`) ?
- Orientation corrigée par `flip` ou `BorrowedOpenGLTextureOrigin` ?
```

- [ ] **Step 2: Mesurer avec le script (il remplit le tableau)**

```bash
./scripts/m0-measure.sh --label "1280×800"   --seconds 30
./scripts/m0-measure.sh --label "pause 10 s" --seconds 12 --pause 2
```

Le redimensionnement 1920×1080 et les 10 seeks restent manuels (interaction souris), puis on relance le script avec `--label "1920×1080"`. Le script donne les chiffres et un pré-diagnostic ; le verdict visuel et la case cochée restent humains.
- [ ] **Step 3: Commit.**

---

## Journal de vérification (2026-11-19, sandbox rootless, sans serveur graphique)

Environnement : Ubuntu 24.04, utilisateur non-root, pas de `sudo`, pas de `$DISPLAY`/`$WAYLAND_DISPLAY`.
Rust 1.100.0 ; Slint **1.19.0** ; libmpv2 **7.0.1** ; libmpv **0.37.0** (extrait localement) ; glow 0.16 (notre dépendance) et 0.18 (interne à Slint, cohabitation sans conflit).

Commande :

```bash
cd spikes/m0-slint-mpv
export LIBRARY_PATH=$PWD/../../vendor/libmpv/usr/lib/x86_64-linux-gnu
export PKG_CONFIG_PATH=$PWD/../../vendor/pkgconfig
cargo check
```

Résultat : `Finished dev profile [unoptimized + debuginfo] target(s) in 0.90s`, **zéro warning**.

Tests purs exécutés sans libmpv (crate `m0-core`, aucune dépendance) :

```bash
cd spikes/m0-core
env -u LIBRARY_PATH -u LD_LIBRARY_PATH -u PKG_CONFIG_PATH cargo test
# running 4 tests ... test result: ok. 4 passed; 0 failed; 0 ignored
ldd <binaire de test>
# linux-vdso.so.1 / libgcc_s.so.1 / libc.so.6 — aucune libmpv
```

Script de mesure testé sur des CSV factices (`--csv` + `--samples`) : ligne obtenue `| TEST | 58 | 59 | 2.3 | 12 | 205 |`, insérée dans un tableau de verdict sans toucher aux autres lignes.

Erreurs réelles trouvées puis corrigées (elles sont reprises dans le code ci-dessus) :

| # | Erreur | Cause | Correction |
|---|---|---|---|
| 1 | `E0277: String is not std::error::Error` | `glow::create_texture()`/`create_framebuffer()` renvoient `Result<_, String>` | `map_err(|e| anyhow!(...))` |
| 2 | `E0277: *mut () cannot be sent between threads` | `slint::Weak` non `Send` dans `invoke_from_event_loop` depuis le callback de frame | flag `Arc<AtomicBool>` + timer UI de redraw (8 ms) |
| 3 | `E0277: *mut () cannot be shared between threads` | `slint::Image` non `Send`, transportée par `invoke_from_event_loop` | slot `Rc<RefCell<Option<UiUpdate>>>` lu sur le thread UI |

Non vérifié ici : édition de liens du binaire graphique, EGL/GLX, rendu effectif, fps réels et `#12030` — tout cela exige Wayland. Les tests purs, eux, tournent sans libmpv.

## Après M0

Le plan M1 sera écrit après le verdict, car il dépend du renderer retenu :

- **M1** : workspace `youtus-*`, shell Slint, OAuth device flow, `browse` accueil/abonnements, miniatures, cache SQLite.
- **S1 (parallèle, sans GUI)** : sonde de capacités OAuth (accueil/historique/like/`playbackTracking`/`createCommentParams`).
- **M2** : page Watch, yt-dlp, lecteur validé en M0.
