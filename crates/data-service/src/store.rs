//! The event log: a SQLite database that only this service writes to.

use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use thiserror::Error;

/// Schema changes, applied in order. The database's `user_version` records
/// how many have been applied. Only ever add to the end of this list.
const MIGRATIONS: &[&str] = &[
    //1. the event log
    "CREATE TABLE events (
        id          TEXT PRIMARY KEY,
        topic       TEXT NOT NULL,
        ts          INTEGER NOT NULL,
        received_at INTEGER NOT NULL,
        payload     TEXT NOT NULL
    );
    CREATE INDEX events_topic_ts ON events(topic, ts);",
];

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database is at schema version {found}, but this build only knows up to {known}")]
    TooNew { found: u32, known: u32 },
}

pub struct Event<'a> {
    pub id: &'a str,
    pub topic: &'a str,
    pub ts: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub payload: &'a str,
}

pub struct Store {
    db: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut db: Connection) -> Result<Self, StoreError> {
        // WAL lets readers use the file while writes occur
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "FULL")?;
        migrate(&mut db)?;
        Ok(Self { db })
    }

    pub fn insert(&self, event: &Event) -> Result<bool, StoreError> {
        let mut statement = self.db.prepare_cached(
            "INSERT OR IGNORE INTO events (id, topic, ts, received_at, payload)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let inserted = statement.execute(params![
            event.id,
            event.topic,
            event.ts.timestamp_millis(),
            event.received_at.timestamp_millis(),
            event.payload
        ])?;
        Ok(inserted == 1)
    }

    pub fn latest_by_topic(&self, prefix: &str) -> Result<Vec<(String, String)>, StoreError> {
        // SQLite fills the other columns from the row holding MAX(ts).
        let mut statement = self.db.prepare(
            "SELECT topic, payload, MAX(ts) FROM events
             WHERE substr(topic, 1, length(?1)) = ?1
             GROUP BY topic",
        )?;
        let rows = statement.query_map([prefix], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let latest = rows.collect::<Result<Vec<_>, _>>()?;
        Ok(latest)
    }
}

fn migrate(db: &mut Connection) -> Result<(), StoreError> {
    let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let known = MIGRATIONS.len() as u32;
    if version > known {
        return Err(StoreError::TooNew {
            found: version,
            known,
        });
    }

    for (index, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let transaction = db.transaction()?;
        transaction.execute_batch(sql)?;
        transaction.pragma_update(None, "user_version", index as u32 + 1)?;
        transaction.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event<'a>(id: &'a str, topic: &'a str, ts_millis: i64, payload: &'a str) -> Event<'a> {
        let ts = DateTime::from_timestamp_millis(ts_millis).unwrap();
        Event {
            id,
            topic,
            ts,
            received_at: ts,
            payload,
        }
    }

    fn count(store: &Store) -> i64 {
        store
            .db
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn creates_the_schema() {
        let store = Store::open_in_memory().unwrap();
        let version: u32 = store
            .db
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();

        assert_eq!(version, MIGRATIONS.len() as u32);
        assert_eq!(count(&store), 0);
    }

    #[test]
    fn migrating_twice_changes_nothing() {
        let mut store = Store::open_in_memory().unwrap();
        migrate(&mut store.db).unwrap();
    }

    #[test]
    fn refuses_a_newer_database() {
        let mut db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "user_version", 99).unwrap();

        let error = migrate(&mut db).unwrap_err();
        assert!(matches!(error, StoreError::TooNew { found: 99, .. }));
    }

    #[test]
    fn opens_a_file_in_wal_mode() {
        let path = std::env::temp_dir().join(format!("nerve-store-{}.db", std::process::id()));
        let store = Store::open(&path).unwrap();
        let mode: String = store
            .db
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();

        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
        assert_eq!(mode, "wal");
    }

    #[test]
    fn skips_duplicate_ids() {
        let store = Store::open_in_memory().unwrap();
        let first = event("a", "home/weather/forecast", 1000, "{}");

        assert!(store.insert(&first).unwrap());
        assert!(!store.insert(&first).unwrap());
        assert_eq!(count(&store), 1);
    }

    #[test]
    fn finds_the_latest_payload_per_topic() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert(&event("1", "home/service/poller-tfl", 1000, "tfl old"))
            .unwrap();
        store
            .insert(&event("2", "home/service/poller-tfl", 3000, "tfl new"))
            .unwrap();
        store
            .insert(&event("3", "home/service/poller-weather", 2000, "weather"))
            .unwrap();
        store
            .insert(&event("4", "home/tfl/status", 4000, "not a service"))
            .unwrap();

        let mut latest = store.latest_by_topic("home/service/").unwrap();
        latest.sort();

        assert_eq!(
            latest,
            [
                ("home/service/poller-tfl".to_string(), "tfl new".to_string()),
                (
                    "home/service/poller-weather".to_string(),
                    "weather".to_string()
                ),
            ]
        );
    }
}
