-- Comptes connus : un refresh token par client OAuth.
CREATE TABLE IF NOT EXISTS accounts (
    id         INTEGER PRIMARY KEY,
    client_id  TEXT NOT NULL UNIQUE,
    label      TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Cache de feeds : permet d'afficher l'accueil immédiatement au démarrage.
CREATE TABLE IF NOT EXISTS feed_cache (
    key        TEXT PRIMARY KEY,
    body       TEXT NOT NULL,
    fetched_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Position de lecture, remplace le progress.json des spikes.
CREATE TABLE IF NOT EXISTS playback (
    video_id    TEXT PRIMARY KEY,
    position_ms INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch())
);
