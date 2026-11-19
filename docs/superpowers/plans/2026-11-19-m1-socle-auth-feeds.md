# M1 — Socle applicatif : workspace, authentification, accueil, abonnements

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development ou superpowers:executing-plans. Cases `- [ ]` pour le suivi.

**Goal:** Passer des spikes à une application qui démarre, s'authentifie, charge l'accueil et les abonnements, et affiche des miniatures — sans jamais bloquer le thread UI.

**Architecture:** Un workspace de cinq crates à dépendances descendantes strictes : `youtus-core` (modèles purs) ← `youtus-yt` (InnerTube + `AuthStrategy`) ← `youtus-auth` (OAuth/cookies) ← `youtus-store` (SQLite) ← `youtus-ui` (Slint). Le binaire `youtus` est le seul point de composition. Toute I/O réseau ou disque passe par un worker unique ; l'UI ne reçoit que des messages déjà décodés.

**Tech Stack:** Rust 2021, Slint 1.19 (winit + FemtoVG), ureq 2 (TLS rustls), serde/serde_json, rusqlite (bundled), image (décodage miniatures), anyhow.

**Spec:** `docs/superpowers/specs/2026-11-19-youtus-design.md` (§5.3–5.5 pour les capacités mesurées)

## Ce que S1 impose à M1

| Contrainte | Origine | Conséquence de conception |
|---|---|---|
| Le client TV rend l'accueil, l'historique et le like | S1, mesuré | `AuthStrategy::Bearer` suffit pour les feeds |
| Le client TV ne rend **ni flux vidéo ni `createCommentParams`** | S1, mesuré | le player et les commentaires passent par `Sapisid` (cookies) |
| `account/account_menu` renvoie 400 même avec un jeton valide | S1, mesuré | jamais utilisé comme test d'authentification |
| Un jeton est lié à un type de client (400 en WEB, 200 en TVHTML5) | S1, mesuré | le profil est choisi **par capacité**, pas globalement |
| `playbackTracking` non conclu (IP de datacenter) | S1, limite | M1 ne construit pas la progression ; M2 la mesure sur la machine réelle |

## Global Constraints

- Linux, Wayland (X11/XWayland en repli). **Jamais `--wid`** : libmpv par API render.
- Slint 1.19.x, backend winit, renderer FemtoVG.
- Aucun appel réseau sur le thread UI. Un seul worker, un canal `crossbeam-channel` ou `std::sync::mpsc`, messages typés.
- Aucun `unwrap()` sur une entrée externe (JSON YouTube, fichier de jeton, cookie).
- Messages UI en français ; identifiants et chemins en anglais.
- Secrets : jamais dans le dépôt, jamais dans un log. Fichier de jeton en `0600`.
- Pas de nouvelle dépendance sans qu'elle soit justifiée dans le commit.

---

## Tâche 1 — Workspace

**Fichiers :** `Cargo.toml`, `rust-toolchain.toml`, `crates/*/Cargo.toml`

- [ ] Créer le workspace virtuel (`resolver = "2"`), membres `crates/youtus-core`, `crates/youtus-yt`, `crates/youtus-auth`, `crates/youtus-store`, `crates/youtus-ui`, `crates/youtus`.
- [ ] Versions partagées dans `[workspace.dependencies]` : slint, ureq, serde, serde_json, anyhow, rusqlite, image.
- [ ] `cargo check --workspace` doit passer avec six crates vides.

**Critère :** `cargo metadata --no-deps` liste les six crates, aucune ne dépend d'une crate de rang supérieur.

## Tâche 2 — `youtus-core` : modèles

**Fichiers :** `crates/youtus-core/src/{lib,model,time}.rs`

- [ ] `VideoId(String)` avec `is_valid()` (11 caractères `[A-Za-z0-9_-]`).
- [ ] `Video { id, title, channel, channel_id, duration: Option<Duration>, views: Option<u64>, published: Option<String>, thumbnail: Option<Url> }`.
- [ ] `Feed { items: Vec<Video>, continuation: Option<String> }`.
- [ ] `sanitize_time` : reprendre `spikes/m0-core` (testé) pour formater `1:02:03`.
- [ ] Tests : id invalide rejeté, durée formatée, feed vide.

**Critère :** `cargo test -p youtus-core` vert, zéro dépendance réseau/UI.

## Tâche 3 — `youtus-yt` : client InnerTube

**Fichiers :** `crates/youtus-yt/src/{lib,client,profile,auth,parse}.rs`

- [ ] `enum AuthStrategy { Anonymous, Bearer { access_token: String }, Sapisid { cookie: String } }`.
- [ ] `Profile { client_name, client_version, header_name, user_agent, platform }` + `Profile::web()` et `Profile::tvhtml5()`.
- [ ] `Client::new(profile, auth)` ; `post(endpoint, body)` ajoute le contexte, les en-têtes, et l'auth **selon la stratégie** (`Authorization: Bearer`, ou `Cookie: SAPISID=…; __Secure-3PAPISID=…` + `Authorization: SAPISIDHASH …` pour WEB).
- [ ] `visitor_data` : récupéré une fois par client et réutilisé (corps + `X-Goog-Visitor-Id`).
- [ ] `browse(browse_id, continuation)` → `Feed`, avec un parseur qui accepte **les trois familles de rendu** : `videoRenderer`, `lockupViewModel`, `tileRenderer` (S1 a montré que le TV n'utilise que le dernier).
- [ ] `like(video_id)` / `unlike(video_id)` ; `like` renvoie `likeStatus`.
- [ ] Erreurs typées : `Http(status)`, `AuthExpired`, `Parse`, pas de `bail!` générique.
- [ ] Tests unitaires sur des JSON figés (`tests/fixtures/tv_home.json`, `web_home.json`) : 20 tuiles TV → 20 vidéos, 0 tuile → feed vide + `continuation`.

**Critère :** `cargo test -p youtus-yt` vert sans réseau. Aucune référence à `account/account_menu` comme test d'auth.

## Tâche 4 — `youtus-auth` : device flow et cookies

**Fichiers :** `crates/youtus-auth/src/{lib,device,token,cookie}.rs`

- [ ] Device flow (déplacé depuis `spikes/s1-oauth/src/oauth.rs`, déjà testé) : `request_code`, `poll`, `refresh`.
- [ ] `StoredToken { client_id, refresh_token }` en `$XDG_DATA_HOME/youtus/oauth.json`, `0600`, écriture atomique (fichier temporaire + `rename`).
- [ ] `refresh` distingue `invalid_grant` (→ `AuthExpired`, l'UI propose de se reconnecter) de `invalid_client` (→ message de configuration).
- [ ] Import de `cookies.txt` (format Netscape) : extraire `SAPISID`/`__Secure-3PAPISID`, refuser un fichier sans ces clés, ne jamais journaliser les valeurs.
- [ ] Tests : parseur de cookies (fichier factice), détection `invalid_grant` sur un JSON d'erreur, permissions du fichier de jeton.

**Critère :** `cargo test -p youtus-auth` vert ; aucune valeur de cookie dans les `Debug`.

## Tâche 5 — `youtus-store` : SQLite

**Fichiers :** `crates/youtus-store/src/{lib,migrations}.rs`, `migrations/0001_init.sql`

- [ ] `rusqlite` avec `bundled`, base dans `$XDG_DATA_HOME/youtus/youtus.db`.
- [ ] Tables : `accounts(id, client_id, label, created_at)`, `settings(key, value)`, `feed_cache(key, body, fetched_at)`, `playback(video_id, position_ms, duration_ms, updated_at)`.
- [ ] Migration idempotente (`user_version`), ouverture en `journal_mode=WAL`.
- [ ] `Playback::save`/`load` (remplace le `progress.json` du spike).
- [ ] Tests : migration sur base neuve, upsert de progression, relecture.

**Critère :** `cargo test -p youtus-store` vert ; base supprimable sans perte de configuration.

## Tâche 6 — `youtus-ui` : coquille Slint

**Fichiers :** `crates/youtus-ui/ui/{app,sidebar,grid,video_card}.slint`, `src/{lib,models}.rs`

- [ ] Fenêtre 1280×800 : barre latérale (Accueil, Abonnements, Historique, Playlists, Recherche) + zone de contenu.
- [ ] `VideoCard` : miniature, titre sur deux lignes, chaîne, durée, vues.
- [ ] `FeedView` : grille responsive (`min-width` 260 px), état « chargement », état « erreur » avec bouton Réessayer, état vide explicite.
- [ ] Modèles Rust : `VecModel<VideoCardData>`, `ModelRc`.
- [ ] Aucune logique réseau dans l'UI : elle envoie `Command::LoadFeed(FeedKind)` et reçoit `Event::Feed(Result<Feed, String>)`.

**Critère :** l'UI se lance avec un feed factice de 12 cartes ; aucune image réseau n'est chargée sur le thread UI.

## Tâche 7 — Miniatures

**Fichiers :** `crates/youtus-ui/src/thumbnails.rs`

- [ ] Worker dédié (2 threads max) : `ureq` → `image::load_from_memory` → redimensionnement 320×180 → `slint::Image`.
- [ ] Cache LRU en mémoire (128 entrées) + repli sur une image grise locale si le décodage échoue.
- [ ] Jamais de `slint::Image` créée hors du thread UI : le worker renvoie des octets RGBA, l'UI construit l'image.

**Critère :** 20 miniatures s'affichent, une URL cassée ne fait pas tomber l'app.

## Tâche 8 — Binaire `youtus` et câblage

**Fichiers :** `crates/youtus/src/main.rs`

- [ ] Charger la config, ouvrir la base, construire le worker, lancer l'UI.
- [ ] Premier démarrage : écran de connexion (device flow) ou import de cookies ; deuxième démarrage : session restaurée.
- [ ] `AuthExpired` → bandeau « Session expirée, reconnecte-toi » + bouton.
- [ ] `--client-secret-file` en argument, jamais en dur.

**Critère :** `cargo run -p youtus` affiche l'accueil réel avec un jeton valide, et l'écran de connexion sans jeton.

---

## Vérification finale

- [ ] `cargo check --workspace` et `cargo test --workspace` verts.
- [ ] `cargo run -p youtus` sur la machine de l'utilisateur : accueil 20 cartes, abonnements, historique.
- [ ] Relancer `spikes/s1-oauth` depuis la machine réelle pour trancher `playbackTracking` (le player est `UNPLAYABLE` depuis une IP de datacenter).
- [ ] Vérifier qu'aucun jeton ni cookie n'apparaît dans `git grep -i "sapisid\|refresh_token"` hors du code de stockage.

## Hors périmètre M1

Lecture vidéo (M2, réutilise le spike M0), progression et beacons (M2, dépend de la mesure `playbackTracking`), publication de commentaires (M3, dépend de `createCommentParams`), playlists et recherche (M3).
