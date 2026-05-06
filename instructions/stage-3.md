# Stage 3: Price Tracking — Implementation Instructions

## 1. Add rusqlite to Cargo.toml

Add to `[dependencies]`:
```toml
rusqlite = { version = "0.31", features = ["bundled"] }
```

## 2. Create `src/db/mod.rs` — DB Init & Migration

```rust
use rusqlite::Connection;
use std::path::Path;
use tracing::{info, error};

pub mod tracking;

pub struct Db {
    pub conn: Connection,
}

impl Db {
    /// Open DB at path, enable WAL, run migrations. Returns None on failure.
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
        let version: i64 = conn.query_row("SELECT version FROM schema_version", [], |r| r.get(0))?;
        info!(schema_version = version, path = %path.display(), "DB initialized");
        Ok(Self { conn })
    }

    fn migrate(conn: &Connection) -> anyhow::Result<()> {
        conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);")?;
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
        Ok(())
    }
}

const SCHEMA_V1: &str = r#"
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
```

Key points:
- `Db::init()` returns `Option<Self>` — caller stores as `Option<Db>`
- WAL mode set immediately after open
- `schema_version` table bootstrapped first, then versioned migrations
- All tables use `IF NOT EXISTS` for safety

## 3. Create `src/db/tracking.rs` — Tracking CRUD (Transactional)

All public functions take `&Connection` (borrowed from `Db.conn`).

### `track_item` — Single Transaction

```rust
pub fn track_item(conn: &Connection, listing: &Listing, notes: Option<&str>) -> anyhow::Result<TrackedItemRow> {
    let tx = conn.transaction()?; // BEGIN IMMEDIATE
    let now = Utc::now().to_rfc3339();

    // 1. Upsert listing
    tx.execute(
        "INSERT OR REPLACE INTO listings (id, platform, title, price, url, city, state, first_seen_at, last_fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, COALESCE((SELECT first_seen_at FROM listings WHERE id=?1), ?8), ?9)",
        params![listing.id, listing.platform.to_str(), listing.title, listing.price, listing.url, listing.city, listing.state, &now, &now],
    )?;

    // 2. Check idempotency — if already tracked, return existing
    if let Some(row) = get_tracked_item_by_listing(&tx, &listing.id)? {
        tx.commit()?;
        return Ok(row);
    }

    // 3. Insert tracked_items
    tx.execute(
        "INSERT INTO tracked_items (listing_id, platform, notes, first_seen_price, current_price, first_seen_at, last_checked_at, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active')",
        params![listing.id, listing.platform.to_str(), notes, listing.price, listing.price, &now, &now],
    )?;

    // 4. Insert initial price snapshot
    tx.execute(
        "INSERT INTO price_snapshots (listing_id, price, recorded_at) VALUES (?1, ?2, ?3)",
        params![listing.id, listing.price, &now],
    )?;

    tx.commit()?;
    get_tracked_item_by_listing(conn, &listing.id)?.ok_or_else(|| anyhow::anyhow!("insert failed"))
}
```

### `untrack_item`

```rust
pub fn untrack_item(conn: &Connection, listing_id: &str) -> anyhow::Result<bool> {
    let deleted = conn.execute("DELETE FROM tracked_items WHERE listing_id = ?1", params![listing_id])?;
    Ok(deleted > 0)
}
```

### `list_tracked_items` — Single JOIN Query

```rust
pub fn list_tracked_items(conn: &Connection) -> anyhow::Result<Vec<TrackedItemRow>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.listing_id, t.platform, t.notes, t.first_seen_price, t.current_price,
                t.first_seen_at, t.last_checked_at, t.status, t.status_changed_at, t.sold_price,
                l.title, l.url
         FROM tracked_items t
         JOIN listings l ON t.listing_id = l.id
         ORDER BY t.first_seen_at DESC"
    )?;
    // map rows to TrackedItemRow
}
```

### `get_price_history`

```rust
pub fn get_price_history(conn: &Connection, listing_id: &str) -> anyhow::Result<Vec<PriceSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT price, recorded_at FROM price_snapshots WHERE listing_id = ?1 ORDER BY recorded_at ASC"
    )?;
    // map rows
}
```

### `mark_as_sold` — Single Transaction

```rust
pub fn mark_as_sold(conn: &Connection, listing_id: &str, sold_price: Option<f64>) -> anyhow::Result<()> {
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();
    let price = sold_price.or_else(|| get_current_price(&tx, listing_id));

    tx.execute(
        "UPDATE tracked_items SET status = 'sold', status_changed_at = ?1, sold_price = ?2 WHERE listing_id = ?3 AND status = 'active'",
        params![&now, price, listing_id],
    )?;

    if let Some(p) = price {
        tx.execute(
            "INSERT INTO price_snapshots (listing_id, price, recorded_at) VALUES (?1, ?2, ?3)",
            params![listing_id, p, &now],
        )?;
    }

    tx.commit()?;
    // Log status transition at INFO
    Ok(())
}
```

### Price Snapshot Deduplication

Before inserting any snapshot, check:
```sql
SELECT 1 FROM price_snapshots
WHERE listing_id = ?1 AND price = ?2
  AND recorded_at > datetime(?3, '-60 seconds')
LIMIT 1
```
If row exists, skip the INSERT.

## 4. Create `src/tools/tracking.rs` — MCP Tool Handlers

Define input structs with `JsonSchema`:

```rust
#[derive(Deserialize, Serialize, JsonSchema)]
pub struct TrackItemInput {
    /// Listing ID to track
    pub listing_id: String,
    /// Platform: "classifieds" or "cars"
    pub platform: String,
    /// Optional notes
    pub notes: Option<String>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct UntrackItemInput { pub listing_id: String }

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct GetPriceHistoryInput { pub listing_id: String }

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct MarkAsSoldInput {
    pub listing_id: String,
    pub sold_price: Option<f64>,
}
```

Each tool method:
1. Checks `self.db.is_some()` — if None, return `"Error: database unavailable (degraded mode)"`
2. Calls the corresponding `db::tracking::*` function
3. Returns JSON-serialized result

`track_item` flow:
1. Fetch listing detail via client (to get current price/title)
2. Call `db::tracking::track_item(conn, &listing, notes)`
3. Return tracked item state

## 5. Update `main.rs` — Graceful Degradation

```rust
// After config load:
let db = Db::init(&config.data_dir.join("ksl.db"));
if db.is_none() {
    tracing::warn!("Running in degraded mode — tracking tools unavailable");
}

// Pass db to server:
let server = KslMcpServer::new(classifieds_client, cars_client, db);
```

`KslMcpServer` gains field: `db: Option<Db>`. Wrap in `Arc<Mutex<Option<Db>>>` since `Connection` is not `Send` — or use `Arc<std::sync::Mutex<Db>>` since rusqlite ops are synchronous and brief.

Recommended: `db: Option<Arc<std::sync::Mutex<Connection>>>` — lock only for the duration of each DB call.

## 6. Add `TrackingStatus` to `src/types.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TrackingStatus {
    Active,
    Sold,
    Removed,
}

impl TrackingStatus {
    pub fn to_str(&self) -> &'static str {
        match self { Self::Active => "active", Self::Sold => "sold", Self::Removed => "removed" }
    }
    pub fn from_str(s: &str) -> Self {
        match s { "sold" => Self::Sold, "removed" => Self::Removed, _ => Self::Active }
    }
}
```

Also add `Platform::to_str()` and `Platform::from_str()` if not present.

## 7. SQL Schema (Exact)

See `SCHEMA_V1` constant in section 2. This is the canonical schema from the spec. No deviations.

## 8. Transaction Boundaries

| Operation | Scope |
|-----------|-------|
| `track_item` | `BEGIN IMMEDIATE` → listings INSERT + tracked_items INSERT + price_snapshots INSERT → `COMMIT` |
| `mark_as_sold` | `BEGIN IMMEDIATE` → tracked_items UPDATE + price_snapshots INSERT → `COMMIT` |
| `mark_as_removed` | `BEGIN IMMEDIATE` → tracked_items UPDATE + price_snapshots INSERT → `COMMIT` |
| Price check update (future) | `BEGIN IMMEDIATE` → price_snapshots INSERT + tracked_items UPDATE → `COMMIT` |

All use `conn.transaction()` which is `BEGIN IMMEDIATE` (write lock acquired upfront). On any error, transaction drops without commit → automatic rollback.

## 9. Idempotency Rules

1. **`track_item`**: After upserting the listing, check `SELECT ... FROM tracked_items WHERE listing_id = ?`. If row exists, return it immediately without inserting a duplicate snapshot. The UNIQUE constraint on `listing_id` is the safety net.

2. **Price snapshot dedup**: Before any snapshot INSERT, query for same `listing_id` + same `price` within last 60 seconds. If found, skip. This prevents duplicate snapshots from rapid repeated calls.

3. **`untrack_item`**: DELETE is naturally idempotent (returns `deleted == 0` if already gone).

## 10. Sold Detection Logic

### Confirmed 404 (Removed)
1. Fetch listing → HTTP 404
2. Wait brief delay, fetch again → HTTP 404
3. Both return 404 → confirmed removed
4. In single transaction: UPDATE `status = 'removed'`, set `status_changed_at`, INSERT final price snapshot with last known price
5. Log at INFO: `listing_id={id} old_status=active new_status=removed trigger=confirmed_404`

### Sold Indicator
1. Fetch listing detail page
2. Parse for sold indicator (e.g., "SOLD" text/badge in listing HTML)
3. If detected → in single transaction: UPDATE `status = 'sold'`, set `status_changed_at`, `sold_price = current_price`, INSERT final snapshot
4. Log at INFO: `listing_id={id} old_status=active new_status=sold trigger=sold_indicator`

### 5xx / Connection Errors — NO Transition
- If fetch returns 5xx or connection timeout/error: log at WARN, do NOT change status
- Rationale: transient server errors must not mark items as removed

### State Machine
```
Active → Sold    (sold indicator OR manual mark_as_sold)
Active → Removed (confirmed 404 — two consecutive 404s)
Sold → (terminal)
Removed → (terminal)
```
No transitions from Sold or Removed back to Active.

## File Checklist

| File | Action |
|------|--------|
| `Cargo.toml` | Add `rusqlite = { version = "0.31", features = ["bundled"] }` |
| `src/db/mod.rs` | Create — DB init, WAL, migration |
| `src/db/tracking.rs` | Create — all CRUD functions |
| `src/tools/tracking.rs` | Create — 5 MCP tool handlers |
| `src/tools/mod.rs` | Add `pub mod tracking;` |
| `src/types.rs` | Add `TrackingStatus` enum, `Platform::to_str/from_str` |
| `src/main.rs` | Add `mod db;`, init DB with graceful degradation, pass to server |
| `src/tools/search.rs` | Update `KslMcpServer` struct to hold `Option<Arc<Mutex<Connection>>>`, add tracking tool methods |
