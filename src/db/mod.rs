use rusqlite::Connection;
use std::path::Path;
use tracing::{error, info};

pub mod searches;
pub mod tracking;

#[cfg(test)]
mod tests;

pub struct Db {
    pub conn: Connection,
}

impl Db {
    pub fn init(path: &Path) -> Option<Self> {
        match Self::try_init(path) {
            Ok(db) => Some(db),
            Err(e) => {
                error!("DB init failed: {e}");
                None
            }
        }
    }

    fn try_init(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Self::migrate(&conn)?;
        let version: i64 =
            conn.query_row("SELECT version FROM schema_version", [], |r| r.get(0))?;
        info!(schema_version = version, path = %path.display(), "DB initialized");
        Ok(Self { conn })
    }

    fn migrate(conn: &Connection) -> anyhow::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);",
        )?;
        let version: i64 = conn
            .query_row("SELECT version FROM schema_version", [], |r| r.get(0))
            .unwrap_or(0);
        if version < 1 {
            conn.execute_batch(SCHEMA_V1)?;
            if version == 0 {
                conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])?;
            } else {
                conn.execute("UPDATE schema_version SET version = 1", [])?;
            }
        }
        if version < 2 {
            conn.execute_batch(SCHEMA_V2)?;
            conn.execute("UPDATE schema_version SET version = 2", [])?;
        }
        Ok(())
    }
}

pub const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS listings (
  id TEXT PRIMARY KEY,
  platform TEXT NOT NULL,
  title TEXT NOT NULL,
  description TEXT,
  price REAL,
  photos TEXT,
  city TEXT, state TEXT, zip TEXT,
  category TEXT, sub_category TEXT,
  seller_type TEXT, posted_date TEXT,
  url TEXT NOT NULL,
  favorites_count INTEGER,
  make TEXT, model TEXT, year INTEGER, mileage INTEGER,
  first_seen_at TEXT NOT NULL,
  last_fetched_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tracked_items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  listing_id TEXT NOT NULL REFERENCES listings(id),
  platform TEXT NOT NULL,
  notes TEXT,
  first_seen_price REAL,
  current_price REAL,
  first_seen_at TEXT NOT NULL,
  last_checked_at TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  status_changed_at TEXT,
  sold_price REAL,
  UNIQUE(listing_id)
);

CREATE TABLE IF NOT EXISTS price_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  listing_id TEXT NOT NULL REFERENCES listings(id),
  price REAL NOT NULL,
  recorded_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_price_snapshots_listing_time ON price_snapshots(listing_id, recorded_at);
"#;

pub const SCHEMA_V2: &str = r#"
CREATE TABLE IF NOT EXISTS saved_searches (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  parameters TEXT NOT NULL,
  platform TEXT NOT NULL,
  last_run_at TEXT,
  created_at TEXT NOT NULL
);
"#;