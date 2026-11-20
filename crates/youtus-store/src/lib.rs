//! Persistance locale : progression de lecture, réglages, cache de feeds.
//!
//! La base vit dans `$XDG_DATA_HOME/youtus/youtus.db` ; la supprimer ne doit
//! coûter que le cache et l'historique local, jamais la session OAuth (qui est
//! dans `oauth.json`).

use std::path::Path;

use rusqlite::{params, Connection};

const SCHEMA: &str = include_str!("../migrations/0001_init.sql");

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("base de données : {0}")]
    Sql(#[from] rusqlite::Error),

    #[error("disque : {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playback {
    pub video_id: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub updated_at: i64,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::prepare(conn)
    }

    pub fn open_in_memory() -> Result<Self, Error> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(conn: Connection) -> Result<Self, Error> {
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn save_playback(
        &self,
        video_id: &str,
        position_ms: u64,
        duration_ms: u64,
    ) -> Result<(), Error> {
        self.conn.execute(
            "INSERT INTO playback (video_id, position_ms, duration_ms, updated_at)
             VALUES (?1, ?2, ?3, unixepoch())
             ON CONFLICT(video_id) DO UPDATE SET
                 position_ms = excluded.position_ms,
                 duration_ms = excluded.duration_ms,
                 updated_at  = excluded.updated_at",
            params![video_id, position_ms as i64, duration_ms as i64],
        )?;
        Ok(())
    }

    pub fn playback(&self, video_id: &str) -> Result<Option<Playback>, Error> {
        let mut statement = self.conn.prepare(
            "SELECT video_id, position_ms, duration_ms, updated_at FROM playback WHERE video_id = ?1",
        )?;
        let mut rows = statement.query([video_id])?;

        match rows.next()? {
            Some(row) => Ok(Some(Playback {
                video_id: row.get(0)?,
                position_ms: row.get::<_, i64>(1)?.max(0) as u64,
                duration_ms: row.get::<_, i64>(2)?.max(0) as u64,
                updated_at: row.get(3)?,
            })),
            None => Ok(None),
        }
    }

    /// Reprend la lecture à la dernière position connue, sans repartir de zéro.
    pub fn resume_position(&self, video_id: &str) -> Result<Option<u64>, Error> {
        Ok(self.playback(video_id)?.map(|playback| playback.position_ms))
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), Error> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, Error> {
        let mut statement = self.conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = statement.query([key])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    pub fn cache_feed(&self, key: &str, body: &str) -> Result<(), Error> {
        self.conn.execute(
            "INSERT INTO feed_cache (key, body, fetched_at) VALUES (?1, ?2, unixepoch())
             ON CONFLICT(key) DO UPDATE SET body = excluded.body, fetched_at = excluded.fetched_at",
            params![key, body],
        )?;
        Ok(())
    }

    pub fn cached_feed(&self, key: &str, max_age_secs: u64) -> Result<Option<String>, Error> {
        let mut statement = self.conn.prepare(
            "SELECT body FROM feed_cache
             WHERE key = ?1 AND fetched_at > unixepoch() - ?2",
        )?;
        let mut rows = statement.query(params![key, max_age_secs as i64])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    pub fn clear_cache(&self) -> Result<(), Error> {
        self.conn.execute("DELETE FROM feed_cache", [])?;
        Ok(())
    }
}

fn migrate(conn: &Connection) -> Result<(), Error> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 1 {
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", 1)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_migration_est_idempotente() {
        let store = Store::open_in_memory().unwrap();
        migrate(&store.conn).unwrap();
        migrate(&store.conn).unwrap();
        let version: i64 = store
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn la_progression_se_met_a_jour() {
        let store = Store::open_in_memory().unwrap();
        store.save_playback("dQw4w9WgXcQ", 1_000, 212_000).unwrap();
        store.save_playback("dQw4w9WgXcQ", 42_000, 212_000).unwrap();

        let playback = store.playback("dQw4w9WgXcQ").unwrap().unwrap();
        assert_eq!(playback.position_ms, 42_000);
        assert_eq!(playback.duration_ms, 212_000);
        assert_eq!(
            store.resume_position("dQw4w9WgXcQ").unwrap(),
            Some(42_000)
        );
    }

    #[test]
    fn une_video_inconnue_n_a_pas_de_position() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.resume_position("abcdefghijk").unwrap(), None);
    }

    #[test]
    fn les_reglages_survivent_a_une_reecriture() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("volume", "70").unwrap();
        store.set_setting("volume", "45").unwrap();
        assert_eq!(store.setting("volume").unwrap().as_deref(), Some("45"));
        assert_eq!(store.setting("absent").unwrap(), None);
    }

    #[test]
    fn le_cache_respecte_la_peremption() {
        let store = Store::open_in_memory().unwrap();
        store.cache_feed("home", "{}").unwrap();
        assert_eq!(store.cached_feed("home", 60).unwrap().as_deref(), Some("{}"));
        assert_eq!(store.cached_feed("home", 0).unwrap(), None);

        store.clear_cache().unwrap();
        assert_eq!(store.cached_feed("home", 60).unwrap(), None);
    }
}
