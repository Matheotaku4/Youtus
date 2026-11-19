# Youtus — spécification de conception

**Date :** 2026-11-19
**Statut :** validée (design approuvé en chat)
**Plateforme :** Linux, Wayland (X11/XWayland supportés si possible), Rust
**But :** client YouTube natif et personnel : accueil personnalisé, abonnements, historique, playlists, lecture mpv intégrée avec skin « façon YouTube », progression transmise à YouTube, like/dislike, abonnement, commentaires.

---

## 1. Décisions verrouillées

| Sujet | Décision | Raison |
|---|---|---|
| UI | **Slint 1.19.x** (`backend-winit` + `renderer-femtovg`) | Choix utilisateur : Rust natif, binaire unique, léger |
| Session cible | **Wayland** en priorité | `--wid` y est impossible ; rendu via API render obligatoire |
| Vidéo | **libmpv2 7.x**, API render OpenGL → FBO → texture Slint | Vidéo = élément d'UI, contrôles superposables, pas de `--wid` |
| Extraction | **yt-dlp** en sous-processus (`-J`) | SABR / PO tokens : ne jamais réimplémenter |
| Auth | **OAuth device flow (TV) en priorité**, **cookies en option débloquante** | OAuth ne couvre pas tout (voir §5) |
| Données | SQLite (cache + positions) ; secrets en keyring | Local-first, aucune donnée envoyée ailleurs |
| Cycle de vie mpv | handle `&'static Mpv` (singleton `Box::leak`) | permet de stocker un `RenderContext<'static>` sans auto-référence ; un seul lecteur par process |
| API | InnerTube (`/youtubei/v1/*`) uniquement | Seule API qui expose accueil/historique/progression |

**Nom des crates :** préfixe `youtus-`, binaire `youtus`.

---

## 2. Périmètre

### Dans le périmètre (v1)

- Accueil personnalisé (`FEwhat_to_watch`), Abonnements (`FEsubscriptions`), Historique (`FEhistory`), Bibliothèque/Playlists (`FElibrary`, `VL<playlistId>`).
- Recherche (`/search`) avec pagination.
- Page Watch : métadonnées, like/dislike, abonnement, description, commentaires (lecture + envoi), colonne « Suivant ».
- Lecture mpv intégrée : play/pause, seek, volume, vitesse, qualité, sous-titres/pistes, plein écran, mode théâtre, raccourcis clavier YouTube.
- Reprise locale + transmission de la progression à YouTube.
- Auth OAuth device flow, import cookies optionnel, écran de capacités.
- Cache hors-ligne des métadonnées (les flux vidéo ne sont pas téléchargés).

### Hors périmètre (assumé)

- Shorts, live chat, téléchargement de vidéos, cast, multi-comptes, DRM/films achetés, mobile, Windows/macOS, i18n au-delà du français.
- Embedding par `--wid` (interdit sous Wayland, écarté partout).

---

## 3. Architecture

```
┌───────────────────────────── youtus-app (binaire) ─────────────────────────────┐
│  boucle Slint (UI thread)  ◄── invoke_from_event_loop ──  runtime tokio        │
│  câblage view-models ↔ services, keyring, chemins XDG                          │
└───────────────┬──────────────────────────┬──────────────────────┬─────────────┘
                │                          │                      │
        ┌───────▼────────┐        ┌────────▼────────┐    ┌────────▼────────┐
        │   youtus-ui    │        │   youtus-play   │    │   youtus-yt     │
        │ *.slint + VMs  │        │ ytdlp.rs mpv.rs │    │ InnerTube pur   │
        │ modèles, cache │        │ trait Player    │    │ auth, feeds,    │
        │ miniatures     │        │                 │    │ actions, stats  │
        └───────┬────────┘        └────────┬────────┘    └────────┬────────┘
                │                          │                      │
        ┌───────▼──────────────────────────▼──────────────────────▼────────┐
        │                          youtus-store                             │
        │      SQLite (cache, positions, réglages) · keyring (secrets)      │
        └───────────────────────────────────────────────────────────────────┘
```

### Règles de dépendance

- `youtus-yt` ne dépend **d'aucun** toolkit graphique ni de Slint. Il est testable avec des fixtures JSON.
- `youtus-play` ne dépend pas de Slint : il expose `trait Player` + un backend `MpvPlayer`. Le contexte GL lui est fourni par l'app (callback `get_proc_address`).
- `youtus-ui` ne fait **aucune I/O réseau** : elle émet des intentions (`slint::callback`) et reçoit des modèles.
- `youtus-store` est le seul à toucher SQLite et le keyring.
- Aucun `unwrap()` sur une donnée venue du réseau.

### Flux d'une action UI

1. Callback Slint (`home-refresh()`) → `youtus-app::actions`.
2. `tokio::spawn` → `youtus-yt::browse::home(&client)`.
3. Résultat → `slint::invoke_from_event_loop` → mise à jour du modèle (`ModelRc<VideoCard>`).
4. Erreur → `AppState.error: string` (bandeau non bloquant) + log.

---

## 4. Couche vidéo

### 4.1 Pipeline de rendu

1. `Window::set_rendering_notifier(...)` (Slint).
2. `RenderingState::RenderingSetup` + `GraphicsAPI::NativeOpenGL { get_proc_address }` :
   - construire un `glow::Context` depuis `get_proc_address` ;
   - créer une texture `GL_TEXTURE_2D` RGBA 1280×720 + un FBO ;
   - créer `Mpv` (`Mpv::with_initializer`) puis `mpv.create_render_context([ApiType(OpenGl), InitParams(OpenGLInitParams { get_proc_address, ctx })])` ;
   - `BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(texture_id, size).build()` → `slint::Image` affectée à la page Watch.
3. `RenderContext::set_update_callback(|| pending.store(true))` — le callback vient d'un thread mpv : **ni API mpv, ni API Slint** dedans. Un `slint::Timer` de 8 ms, côté UI, consomme le drapeau et appelle `request_redraw()`.
4. `RenderingState::BeforeRendering` : si `frame_pending`, `render_context.render::<LoaderCtx>(fbo_id, 1280, 720, false)` puis `report_swap()`. **`flip = false`** : avec `true`, mpv écrit la texture avec l'origine en bas (convention OpenGL) et Slint l'affiche à l'envers, puisque `BorrowedOpenGLTextureOrigin::TopLeft` est son défaut. Vérifié à l'écran sur KDE/Wayland.
5. `RenderingState::RenderingTeardown` : drop du render context avant le contexte mpv, puis texture/FBO.

La taille de texture est fixe en v1 (le scaling est fait par Slint). Le redimensionnement du FBO est une optimisation ultérieure.

**Contrainte de threading vérifiée à la compilation :** `slint::Image` et `slint::Weak` ne sont pas `Send`. L'image issue du notifier ne peut donc pas être transmise par `slint::invoke_from_event_loop` ni par un canal : elle transite par un slot `Rc<RefCell<Option<UiUpdate>>>` (type `Frame(image, status)` ou `Status(message)`) lu par le timer UI, hors phase de rendu. Corollaire : la mise à jour d'état du lecteur (position, durée, paused) se fait par polling `slint::Timer`, jamais depuis un callback mpv.

### 4.2 Détails d'API confirmés par compilation (2026-11-19)

- `glow::create_texture()` / `create_framebuffer()` renvoient `Result<_, String>` : conversion explicite `map_err` vers `anyhow`.
- `RenderContext::render` a un générique `GLContext` non inférable : appel en turbofish `render::<LoaderCtx>(...)`, avec le **même** type que lors de `create_render_context`.
- `OpenGLInitParams::get_proc_address` est un pointeur de fonction `fn(&GLContext, &str) -> *mut c_void` : le loader Slint (`&dyn Fn(&CStr) -> *const c_void`) doit être enveloppé dans une structure et convertir `&str` → `CString`.
- Stack validée : Rust 1.100, Slint 1.19.0, libmpv2 7.0.1, libmpv 0.37.0.
- Géométrie Slint : `x` et `y` sont **centrés par défaut** (`x = (parent.width - self.width) / 2` d'après la référence « Geometry »), d'où un remplissage de barre de progression qui paraît centré. Toujours poser `x: 0` sur la piste, le remplissage et la `TouchArea`. Corollaire pour tous les composants Youtus : ne jamais compter sur la position implicite d'un enfant hors layout.
- Mesures de référence (M0, KDE/Wayland, NVIDIA 8 Go) : 60 fps, 0,6 ms/frame, 6 % CPU, 424 Mo RSS pour une texture 1280×720.

### 4.3 Options mpv imposées

```
osc=no, osd-level=0, input-default-bindings=no, input-vo-keyboard=no,
vo=libmpv, hwdec=auto-safe, video-timing-offset=0, keep-open=yes,
ytdl=no, config=no, cache=yes, demuxer-max-bytes=256MiB
```

`ytdl=no` : c'est nous qui résolvons via yt-dlp. `keep-open=yes` : garder la dernière frame après EOF.

### 4.4 Résolution du flux (`youtus-play::ytdlp`)

```rust
pub struct ResolvedStream {
    pub url: String,
    pub http_headers: Vec<(String, String)>,
    pub format_id: String,
    pub height: Option<u32>,
    pub expires_at: SystemTime,
}

pub fn resolve(video_id: &str, max_height: u32, cookies: Option<&Path>) -> Result<ResolvedStream>
```

Commande : `yt-dlp -J --no-warnings --no-playlist --no-progress -f "bv*[height<=H]+ba/b[height<=H]/b" --extractor-args "youtube:player-client=tv,ios,web_safari"` (+ `--cookies` si fourni). Les `http_headers` sont passés à mpv via `http-header-fields`. Cache mémoire 30 min par `(video_id, max_height)`. Si `yt-dlp` est absent : erreur typée `PlayerError::YtDlpMissing` et action UI « installer yt-dlp » (téléchargement du binaire standalone dans `~/.local/share/youtus/bin`).

### 4.5 API `trait Player`

```rust
/// Confiné au thread UI en v1 : le render context GL y vit déjà.
/// Si des blocages apparaissent, les commandes migreront vers un client mpv
/// dédié (`Mpv::create_client`) piloté par canal depuis un thread séparé.
pub trait Player {
    fn load(&self, stream: &ResolvedStream, start_at: Option<f64>) -> Result<()>;
    fn set_paused(&self, paused: bool) -> Result<()>;
    fn seek_absolute(&self, secs: f64) -> Result<()>;
    fn set_volume(&self, v: f64) -> Result<()>;
    fn set_speed(&self, v: f64) -> Result<()>;
    fn set_track(&self, kind: TrackKind, id: i64) -> Result<()>;
    fn snapshot(&self) -> PlaybackSnapshot; // position, durée, paused, buffered, pistes
}
```

`snapshot()` est interrogé toutes les 250 ms par un timer Slint (pas d'observer property mpv en v1 : plus simple, coût négligeable).

### 4.6 Dégradation

| Panne | Comportement |
|---|---|
| Pas de contexte OpenGL (`RenderingSetup` non-OpenGL) | UI fonctionne, zone vidéo remplacée par un message ; lien « ouvrir dans mpv » |
| `#12030` (lent) | Bascule renderer Skia-OpenGL puis, en dernier recours, `MPV_RENDER_API_TYPE_SW` |
| yt-dlp absent / échec | Message + bouton réessayer + bouton « ouvrir l'URL dans mpv » |
| Vidéo non lisible (DRM, âge) | Message issu de `playabilityStatus.reason` |

---

## 5. Authentification et capacités

### 5.1 OAuth device flow (défaut)

1. `POST https://oauth2.googleapis.com/device/code` (repli `https://www.youtube.com/o/oauth2/device/code`) avec `client_id` (type « TV et appareils à accès limité »), `scope=https://www.googleapis.com/auth/youtube` — ce scope couvre déjà la lecture, un `readonly` séparé est inutile —, `device_id`, `device_model=ytlr::`.
2. UI : afficher `user_code` + `verification_url` (youtube.com/activate).
3. Poll `https://oauth2.googleapis.com/token` (`grant_type=urn:ietf:params:oauth:grant-type:device_code`), respecter `interval`.
4. Stocker **uniquement le refresh token** dans le keyring ; access token en mémoire, rafraîchi à 80 % de sa durée.
5. Requêtes personnalisées : `Authorization: Bearer <access_token>`.

### 5.2 Cookies (déblocage optionnel)

- Import d'un `cookies.txt` (Netscape) exporté par l'utilisateur — pas de déchiffrement de profil Chromium.
- Extraire `SAPISID` (ou `__Secure-3PAPISID`), calculer :
  `Authorization: SAPISIDHASH <ts>_<sha1("<ts> <SAPISID> youtube.com")>` (+ variantes 1P/3P), `X-Origin: youtube.com`, `X-Goog-AuthUser: 0`, cookie `LOGIN_INFO` présent.
- Stockage keyring. Le fichier cookies n'est jamais copié dans le dépôt ni loggé.

### 5.3 Matrice de capacités (affichée dans Réglages)

| Capacité | OAuth | Cookies | Mesurée par |
|---|---|---|---|
| Accueil / Abonnements / Historique / Playlists | attendu ✅ | attendu ✅ | `browse FEwhat_to_watch` / `FEhistory` |
| Like, dislike, abonnement | attendu ✅ | attendu ✅ | `like/like` puis `like/removelike` |
| Lire les commentaires | attendu ✅ | attendu ✅ | `next` + continuation `comment-item-section` |
| Publier un commentaire | **attendu ❌** | attendu ✅ | présence de `createCommentParams` |
| Beacons de progression | attendu ✅ | attendu ✅ | `playbackTracking` du `/player` |
| Extraction du flux plus fiable (PO tokens) | ~ | attendu ✅ | hors S1, mesuré en M2 |

Les cellules ci-dessus sont des **prédictions** (documentation Google + retour d'expérience des clients tiers, notamment l'absence de `force-ssl` dans le scope TV qui bloque la publication de commentaires). Ce sont les valeurs mesurées par la sonde S1 (§5.4) qui remplacent ces prédictions et qui sont affichées dans Réglages — jamais l'inverse. Tant qu'un compte n'a pas été sondé, l'app affiche « non vérifié » et non « ✅ ».

La sonde de capacités s'exécute au login et à la demande : `account/account_menu` + `browse FEhistory` + `player` sur une vidéo témoin, et enregistre les résultats par compte. L'UI grise les actions indisponibles avec une infobulle explicite.

### 5.4 Sonde S1 (binaire headless)

`spikes/s1-oauth` est un binaire sans GUI et sans libmpv : device flow, puis mesure des capacités réelles. C'est la **seule source de vérité** pour les cellules de §5.3.

```bash
YOUTUS_OAUTH_CLIENT_ID=<id client TV> cargo run --release \
  --manifest-path spikes/s1-oauth/Cargo.toml -- \
  --markdown docs/superpowers/spikes/2026-11-19-s1-capabilities.md \
  --json /tmp/s1-report.json
```

Création du client OAuth (une fois) : console Google Cloud → projet → activer « YouTube Data API v3 » → écran de consentement externe en mode test avec son compte en testeur → identifiant OAuth de type « TV et périphériques à entrée limitée ».

| Vérification | Endpoint | Critère de succès | Interprétation de l'échec |
|---|---|---|---|
| Authentification Bearer | `account/account_menu` | `accountName` présent | le jeton TV n'est pas accepté par ce contexte client |
| Accueil personnalisé | `browse FEwhat_to_watch` | ≥ 1 `videoRenderer` | feed anonyme ou `feedNudgeRenderer` : personnalisation refusée |
| Historique | `browse FEhistory` | ≥ 1 `videoRenderer` | scope insuffisant ou compte neuf |
| Like | `like/like` puis `like/removelike` | `likeStatus = LIKE` | mutation refusée → bouton grisé en UI |
| Progression | `player` avec `signatureTimestamp` | `videostatsWatchtimeUrl` **et** `videostatsPlaybackUrl` | pas de progression possible sans cookies |
| Commenter | `next` sur la continuation commentaires | `createCommentEndpoint.createCommentParams` non vide | publication impossible en OAuth → cookies requis |

Protocole et pièges :

- `signatureTimestamp` est extrait du player JS courant (HTML watch → `jsUrl` → `signatureTimestamp:<n>`) ; sans lui, `/player` n'inclut pas `playbackTracking`. La sonde distingue « bloc absent avec sts valide » de « sts introuvable » au lieu de conclure à tort.
- Le like est **restauré** systématiquement (`like/removelike`) ; `--skip-like` supprime toute mutation.
- Deux contextes sont essayés (`WEB`, puis `TVHTML5` si `--client` n'est pas fixé) : le jeton vient d'un client TV et n'est pas garanti accepté par `WEB`. Le contexte qui répond est consigné dans le rapport.
- Le refresh token est écrit dans `$XDG_DATA_HOME/youtus/oauth.json` en 0600 (repli `~/.local/share/youtus/`), jamais dans le dépôt ni dans un log.
- Vidéo témoin par défaut `dQw4w9WgXcQ` (commentaires ouverts, likeable), remplaçable par `--video`.

---

## 6. InnerTube

### 6.1 Requête de base

```
POST https://www.youtube.com/youtubei/v1/<endpoint>?prettyPrint=false
Content-Type: application/json
X-YouTube-Client-Name: 1
X-YouTube-Client-Version: <version>
User-Agent: <Chrome desktop>
Origin: https://www.youtube.com
Referer: https://www.youtube.com/
X-Goog-Visitor-Id: <visitorData>
[Authorization: Bearer … | SAPISIDHASH …]
[X-Origin: youtube.com, X-Goog-AuthUser: 0]   // mode cookies uniquement
```

Corps : `{"context":{"client":{"clientName":"WEB","clientVersion":"…","hl":"fr","gl":"FR","visitorData":"…"}}}` + champs de l'endpoint. `visitorData` est conservé depuis `responseContext.visitorData` (persisté en SQLite). Version client : constante `CLIENT_VERSION` dans un seul fichier, mise à jour lors des régressions ; override par variable d'environnement `YOUTUS_CLIENT_VERSION` pour dépanner sans rebuild.

### 6.2 Endpoints utilisés

| Besoin | Endpoint | Champs |
|---|---|---|
| Accueil | `browse` | `browseId: FEwhat_to_watch` |
| Abonnements | `browse` | `browseId: FEsubscriptions` |
| Historique | `browse` | `browseId: FEhistory` |
| Bibliothèque | `browse` | `browseId: FElibrary` |
| Playlist | `browse` | `browseId: "VL" + playlist_id` |
| Chaîne | `browse` | `browseId: UC…`, `params` d'onglet |
| Page watch | `next` | `videoId` |
| Suivant / commentaires | `next` | `continuation` |
| Métadonnées + tracking | `player` | `videoId`, `contentCheckOk`, `racyCheckOk`, `playbackContext.contentPlaybackContext.signatureTimestamp` |
| Recherche | `search` | `query`, `params` |
| Like | `like/like`, `like/removelike` | `target: { videoId }` |
| Abonnement | `subscription/subscribe`, `/unsubscribe` | `channelIds: [UC…]` |
| Commenter | `comment/create_comment` | `createCommentParams`, `richTextBody` |
| Compte | `account/account_menu` | — |

### 6.3 Parsing

Le JSON InnerTube est instable et profondément imbriqué. Règles :

- Structures `serde` **tolérantes** (`#[serde(default)]`, champs inconnus ignorés).
- Un module `extract` par surface, avec un **collecteur récursif** de renderers (`videoRenderer`, `richItemRenderer`, `lockupViewModel`, `playlistVideoRenderer`) plutôt que des chemins fixes.
- Toute réponse est d'abord stockée brute (option `YOUTUS_DUMP_JSON=1`) pour fabriquer une fixture en cas de régression.
- Tests unitaires sur fixtures réelles anonymisées dans `youtus-yt/tests/fixtures/`.

### 6.4 Erreurs

`YtError` : `Network`, `Http(u16)`, `AuthRequired`, `AuthExpired`, `ConsentRequired`, `RateLimited`, `Parse { surface, reason }`, `Upstream(reason)`. Le refresh OAuth est automatique une fois sur `AuthExpired`/401.

---

## 7. Progression vers YouTube

Source : bloc `playbackTracking` du `/player` authentifié.

| URL | Quand | Paramètres |
|---|---|---|
| `videostatsPlaybackUrl` | dès la 1ʳᵉ seconde audible (ou 2 s après `load`) | `cpn`, `ver=2`, `c`, `cver`, `cplayer=UNIPLAYER`, `hl`, `cr` |
| `atrUrl` | à `elapsedMediaTimeSeconds` (défaut 5 s) | `cpn` |
| `videostatsWatchtimeUrl` | toutes les 30 s de **nouvelle** position, et à la fin | `cpn`, `st=0`, `et=<s>`, `cmt=<s>`, `state=playing|paused`, `final=1` à la fermeture |

- Un `cpn` de 16 caractères `[A-Za-z0-9_-]` par lecture, généré cryptographiquement.
- Nouvelle lecture = nouveau `cpn` ; la reprise après seek n'en crée pas.
- Les pings sont *best effort* : jamais bloquants, jamais d'erreur visible, 1 retry.
- Si les URLs sont absentes (non authentifié / tracking refusé) : mode « historique local seulement », signalé dans l'UI.
- À la fermeture de l'app ou changement de vidéo : `final=1&state=paused` avec la dernière position connue.

---

## 8. Interface

### 8.1 Écrans

1. **Shell** : sidebar 240 px (Accueil, Abonnements, Historique, Playlists, Réglages), topbar (recherche, avatar), zone contenu.
2. **Accueil / Abonnements** : grille responsive 1–4 colonnes, cartes 16:9, chips de filtres, scroll infini (continuation), skeletons.
3. **Historique** : liste horizontale, barre de progression par vidéo, regroupement par jour.
4. **Playlists** : liste, en-tête, lecture séquentielle.
5. **Watch** : vidéo 16:9 coin gauche (max 1280 px), titre, ligne chaîne + like/dislike + partager/enregistrer, description repliable, commentaires, colonne droite « Suivant » (168×94) ; mode théâtre ; plein écran.
6. **Réglages** : compte + capacités, qualité max, cookies, yt-dlp, dossier de cache, raccourcis.

### 8.2 Design tokens

```
fond #0f0f0f · surface #212121 · hover #272727 · bordure #303030
texte #f1f1f1 · texte secondaire #aaaaaa · accent #3ea6ff · like #f1f1f1
rayon carte 12px · rayon vignette 8px · gap grille 16px · sidebar 240px
Typo Roboto (Apache-2.0, embarquée) : 28/20/16/14/12 px, titres 500
```

### 8.3 Interactions

- Hover carte : `animate` 120 ms (rayon 12 px, fond #272727), durée affichée en overlay.
- Raccourcis Watch : `k`/espace play, `j`/`l` ±10 s, `←`/`→` ±5 s, `↑`/`↓` volume, `m` mute, `f` plein écran, `t` théâtre, `c` sous-titres, `0–9` seek relatif, `Esc` sortie plein écran.
- Les contrôles apparaissent au mouvement de souris, disparaissent après 2,5 s de lecture.

### 8.4 États obligatoires

Chaque surface a 4 états visuels : chargement (skeleton), vide (message + action), erreur (message + réessayer), hors-ligne (bandeau). Aucun spinner infini : timeout réseau 15 s.

### 8.5 Miniatures

Téléchargement via `reqwest` (pool 6), décodage `image` hors thread UI, redimensionnement à 640×360 max, cache disque `~/.cache/youtus/thumbs/` (clé = hash de l'URL), index dans SQLite, LRU 500 entrées / 200 Mo. `slint::Image::from_rgba8` sur le thread UI.

---

## 9. Données locales

```sql
feeds(key TEXT PRIMARY KEY, payload TEXT NOT NULL, fetched_at INTEGER NOT NULL);
videos(id TEXT PRIMARY KEY, payload TEXT NOT NULL, fetched_at INTEGER NOT NULL);
watch_positions(video_id TEXT PRIMARY KEY, position REAL NOT NULL,
                duration REAL NOT NULL, updated_at INTEGER NOT NULL);
thumbnails(url TEXT PRIMARY KEY, path TEXT NOT NULL, fetched_at INTEGER NOT NULL);
settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
```

TTL : accueil 10 min, abonnements 5 min, historique 5 min, watch 6 h, recherche 10 min, miniatures 30 j. Toute lecture de cache est immédiate puis rafraîchie en arrière-plan.

Secrets (keyring `youtus` / Secret Service) : `oauth_refresh_token`, `cookies_netscape`. Repli si aucun Secret Service : fichier `~/.local/share/youtus/secrets.json` en 0600 avec avertissement UI.

---

## 10. Budgets

| Métrique | Cible |
|---|---|
| UI au repos | 60 fps, < 3 % CPU |
| UI pendant lecture 1080p60 | 60 fps, décodage matériel (`hwdec=auto-safe`) |
| RAM totale 1 fenêtre + lecture | < 350 Mo (mesuré M0 : 424 Mo, marge à trouver en M1) |
| Démarrage à froid (UI affichée) | < 800 ms |
| Accueil affiché (réseau, cache froid) | < 1,5 s |

---

## 11. Tests et vérification

- `cargo test -p youtus-yt` : parsing fixtures (accueil, watch, commentaires, playlists), construction des URLs de stats, `cpn`, SAPISIDHASH (vecteurs figés), machine d'état progression (30 s / final).
- `cargo test -p youtus-play` : parsing `yt-dlp -J` (fixture), choix de format, TTL du cache de résolution.
- `youtus-store` : migrations + upsert positions (base `:memory:`).
- UI : `cargo build` + vérification manuelle documentée par écran (pas de tests d'IHM automatisés en v1).
- Sonde réseau réelle : `#[ignore]` + variables `YOUTUS_LIVE=1`.
- **Règle d'isolation** (validée par le spike M0) : toute logique sans I/O — assainissement des temps, URLs de stats, `cpn`, signature SAPISIDHASH, machine d'état de progression — vit dans un crate sans dépendance système, testable sans libmpv, sans Slint et sans serveur graphique.

**Aucune fonctionnalité n'est « finie » sans sa commande de vérification exécutée et son résultat noté dans le plan.**

---

## 12. Jalons et critères de sortie

| Jalon | Contenu | Critère de sortie |
|---|---|---|
| **M0 — Spike vidéo** | Slint + libmpv FBO/texture + contrôles + mesure fps | 1080p60 Wayland, 60 fps UI, CPU raisonnable, verdict écrit sur `#12030` |
| **M1 — Squelette + accueil** | Workspace, shell Slint, OAuth, `browse` accueil/abonnements, miniatures, cache | Accueil personnalisé affiché avec de vraies données, capacités détectées |
| **M2 — Lecture** | yt-dlp + mpv intégré, page Watch, contrôles, reprise locale | Une vidéo se lance depuis l'accueil et se reprend après redémarrage |
| **M3 — Progression + actions** | beacons, like/dislike, abonnement, cookies | Progression visible dans l'historique YouTube, like persistant |
| **M4 — Commentaires** | lecture, pagination, envoi (cookies), réponses | Commentaire publié depuis l'app et visible sur YouTube |
| **M5 — Playlists / historique / recherche** | listes, lecture séquentielle, recherche paginée | Playlist lue de bout en bout |
| **M6 — Finitions** | théâtre/plein écran, sous-titres, réglages, packaging | `.deb` + AppImage lancés sur une machine vierge |

Les jalons M3+ font l'objet de plans séparés, écrits après le verdict du spike (le renderer retenu peut changer la donne).

---

## 13. Risques et parades

| Risque | Impact | Parade |
|---|---|---|
| `#12030` : notifier FemtoVG lent | 60 fps impossible | Mesure en M0 ; Skia-OpenGL ; dernier recours SW |
| SABR / PO tokens | Lecture cassée | yt-dlp récent auto-mis à jour, clients `tv,ios,web_safari`, cookies, Premium exempté |
| InnerTube change | Écrans vides | parsing tolérant, collecteur récursif, dump JSON, client version overridable |
| OAuth ne permet pas commenter | Fonction M4 incomplète | cookies débloquants + matrice de capacités transparente |
| Slint ≠ navigateur | Fidélité visuelle ~85–90 % | tokens + Roboto + composants dédiés ; ne pas promettre le pixel-perfect |
| Bubble de mapping des commentaires | Blocage M4 | Spike S1 (voir plan M0) : vérifier `create_comment` + `createCommentParams` |
| Licence Slint | GPLv3 si distribution fermée | usage personnel ; décision commerciale hors périmètre |

---

## 14. Sources techniques de référence

- API render mpv : <https://github.com/mpv-player/mpv-examples/blob/master/libmpv/README.md> · `include/mpv/render.h`
- `--wid` interdit sous Wayland : <https://github.com/mpv-player/mpv/issues/9654>
- Slint rendering notifier / OpenGL : <https://docs.slint.dev/latest/docs/rust/slint/enum.GraphicsAPI>
- Slint + texture OpenGL : `examples/opengl_texture` · `BorrowedOpenGLTextureBuilder`
- Perf notifier FemtoVG : <https://github.com/slint-ui/slint/issues/12030>
- libmpv2 7.0.1 : `Mpv::create_render_context`, `RenderContext::render`, `OpenGLInitParams`
- Endpoints InnerTube : <https://github.com/IarChep/MeeTube/blob/master/docs/INNERTUBE_API.md> (voir aussi YouPipe `docs/apis.md`)
- Progression / `playbackTracking` : implémentation de référence BitChord `historyReporting.ts`
- PO tokens / SABR : <https://github.com/yt-dlp/yt-dlp/wiki/Po-Token-Guide>
