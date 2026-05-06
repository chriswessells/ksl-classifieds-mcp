# Stage 5: Saved Searches & Stats — Implementation Instructions

## 1. Schema Migration (src/db/mod.rs)

Add `SCHEMA_V2` constant and update `migrate()` to apply it when version < 2:

```rust
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
```

In `migrate()`, after the `version < 1` block, add:

```rust
if version < 2 {
    conn.execute_batch(SCHEMA_V2)?;
    conn.execute("UPDATE schema_version SET version = 2", [])?;
}
```

## 2. Create src/db/searches.rs — CRUD for Saved Searches

Add `pub mod searches;` to `src/db/mod.rs`.

File contents:

```rust
use anyhow::anyhow;
use chrono::Utc;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::types::{ClassifiedsSearchParams, CarsSearchParams};

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedSearchRow {
    pub id: i64,
    pub name: String,
    pub parameters: String,
    pub platform: String,
    pub last_run_at: Option<String>,
    pub created_at: String,
}

/// Parameters stored as JSON — this enum deserializes from the TEXT column.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "platform")]
pub enum SavedSearchParams {
    #[serde(rename = "classifieds")]
    Classifieds(ClassifiedsSearchParams),
    #[serde(rename = "cars")]
    Cars(CarsSearchParams),
}

pub fn save_search(conn: &Connection, name: &str, params: &SavedSearchParams) -> anyhow::Result<SavedSearchRow> {
    let now = Utc::now().to_rfc3339();
    let platform = match params {
        SavedSearchParams::Classifieds(_) => "classifieds",
        SavedSearchParams::Cars(_) => "cars",
    };
    let json = serde_json::to_string(params)?;
    conn.execute(
        "INSERT INTO saved_searches (name, parameters, platform, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![name, json, platform, &now],
    )?;
    let id = conn.last_insert_rowid();
    get_by_id(conn, id)?.ok_or_else(|| anyhow!("insert failed"))
}

pub fn list_saved_searches(conn: &Connection) -> anyhow::Result<Vec<SavedSearchRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, parameters, platform, last_run_at, created_at FROM saved_searches ORDER BY created_at DESC"
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(SavedSearchRow {
            id: r.get(0)?,
            name: r.get(1)?,
            parameters: r.get(2)?,
            platform: r.get(3)?,
            last_run_at: r.get(4)?,
            created_at: r.get(5)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn delete_saved_search(conn: &Connection, id: i64) -> anyhow::Result<bool> {
    let deleted = conn.execute("DELETE FROM saved_searches WHERE id = ?1", params![id])?;
    Ok(deleted > 0)
}

pub fn update_last_run(conn: &Connection, id: i64) -> anyhow::Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute("UPDATE saved_searches SET last_run_at = ?1 WHERE id = ?2", params![&now, id])?;
    Ok(())
}

pub fn get_by_id(conn: &Connection, id: i64) -> anyhow::Result<Option<SavedSearchRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, parameters, platform, last_run_at, created_at FROM saved_searches WHERE id = ?1"
    )?;
    let mut rows = stmt.query_map(params![id], |r| {
        Ok(SavedSearchRow {
            id: r.get(0)?,
            name: r.get(1)?,
            parameters: r.get(2)?,
            platform: r.get(3)?,
            last_run_at: r.get(4)?,
            created_at: r.get(5)?,
        })
    })?;
    Ok(rows.next().transpose()?)
}

/// Deserialize the JSON parameters column into the typed enum.
pub fn parse_params(row: &SavedSearchRow) -> anyhow::Result<SavedSearchParams> {
    Ok(serde_json::from_str(&row.parameters)?)
}
```

## 3. Create src/tools/searches.rs — Tool Functions

Add `pub mod searches;` to `src/tools/mod.rs`.

```rust
use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::db::searches as db;
use crate::db::searches::SavedSearchParams;
use crate::tools::tracking::DbHandle;
use crate::types::{CarsSearchParams, ClassifiedsSearchParams};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SaveSearchInput {
    /// Name for this saved search
    pub name: String,
    /// Platform: "classifieds" or "cars"
    pub platform: String,
    /// Keyword
    pub keyword: Option<String>,
    /// Category (classifieds only)
    pub category: Option<String>,
    /// Minimum price
    pub price_from: Option<u32>,
    /// Maximum price
    pub price_to: Option<u32>,
    /// ZIP code
    pub zip: Option<String>,
    /// Radius in miles
    pub miles: Option<u32>,
    /// Sort order (classifieds only)
    pub sort: Option<String>,
    /// Car make (cars only)
    pub make: Option<String>,
    /// Car model (cars only)
    pub model: Option<String>,
    /// Min year (cars only)
    pub year_from: Option<u32>,
    /// Max year (cars only)
    pub year_to: Option<u32>,
    /// Min mileage (cars only)
    pub mileage_from: Option<u32>,
    /// Max mileage (cars only)
    pub mileage_to: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DeleteSavedSearchInput {
    /// ID of the saved search to delete
    pub id: i64,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunSavedSearchInput {
    /// ID of the saved search to run
    pub id: i64,
}

fn db_unavailable() -> String {
    r#"{"error":"database unavailable (degraded mode)"}"#.to_string()
}

fn lock_err() -> String {
    r#"{"error":"database lock poisoned"}"#.to_string()
}

pub fn save_search(db_handle: &Option<DbHandle>, input: SaveSearchInput) -> String {
    let Some(handle) = db_handle else { return db_unavailable() };
    let guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return lock_err(),
    };
    let params = build_params(&input);
    match db::save_search(&guard.conn, &input.name, &params) {
        Ok(row) => serde_json::to_string(&row).unwrap_or_else(|e| e.to_string()),
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    }
}

pub fn list_saved_searches(db_handle: &Option<DbHandle>) -> String {
    let Some(handle) = db_handle else { return db_unavailable() };
    let guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return lock_err(),
    };
    match db::list_saved_searches(&guard.conn) {
        Ok(rows) => serde_json::to_string(&rows).unwrap_or_else(|e| e.to_string()),
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    }
}

pub fn delete_saved_search(db_handle: &Option<DbHandle>, input: DeleteSavedSearchInput) -> String {
    let Some(handle) = db_handle else { return db_unavailable() };
    let guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return lock_err(),
    };
    match db::delete_saved_search(&guard.conn, input.id) {
        Ok(removed) => format!(r#"{{"removed":{removed}}}"#),
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    }
}

/// Returns the saved search row (with params) so the caller can execute the search.
/// Updates last_run_at. The actual HTTP search is performed by the caller in search.rs.
pub fn get_saved_search_for_run(db_handle: &Option<DbHandle>, input: &RunSavedSearchInput) -> Result<(db::SavedSearchRow, SavedSearchParams), String> {
    let Some(handle) = db_handle else { return Err(db_unavailable()) };
    let guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return Err(lock_err()),
    };
    let row = match db::get_by_id(&guard.conn, input.id) {
        Ok(Some(r)) => r,
        Ok(None) => return Err(r#"{"error":"saved search not found"}"#.to_string()),
        Err(e) => return Err(format!(r#"{{"error":"{e}"}}"#)),
    };
    let params = match db::parse_params(&row) {
        Ok(p) => p,
        Err(e) => return Err(format!(r#"{{"error":"invalid saved params: {e}"}}"#)),
    };
    let _ = db::update_last_run(&guard.conn, input.id);
    Ok((row, params))
}

fn build_params(input: &SaveSearchInput) -> SavedSearchParams {
    let sort = input.sort.as_deref().and_then(|s| match s {
        "Newest" => Some(crate::types::SortOrder::Newest),
        "Oldest" => Some(crate::types::SortOrder::Oldest),
        "PriceLow" => Some(crate::types::SortOrder::PriceLow),
        "PriceHigh" => Some(crate::types::SortOrder::PriceHigh),
        _ => None,
    });

    if input.platform == "cars" {
        SavedSearchParams::Cars(CarsSearchParams {
            keyword: input.keyword.clone(),
            make: input.make.clone(),
            model: input.model.clone(),
            year_from: input.year_from,
            year_to: input.year_to,
            price_from: input.price_from,
            price_to: input.price_to,
            mileage_from: input.mileage_from,
            mileage_to: input.mileage_to,
            zip: input.zip.clone(),
            miles: input.miles,
            ..Default::default()
        })
    } else {
        SavedSearchParams::Classifieds(ClassifiedsSearchParams {
            keyword: input.keyword.clone(),
            category: input.category.clone(),
            price_from: input.price_from,
            price_to: input.price_to,
            zip: input.zip.clone(),
            miles: input.miles,
            sort,
            ..Default::default()
        })
    }
}
```

## 4. Add get_sales_stats to src/tools/tracking.rs

Add a new input struct and function:

```rust
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetSalesStatsInput {
    /// Optional platform filter: "classifieds" or "cars"
    pub platform: Option<String>,
    /// Optional category filter
    pub category: Option<String>,
}

pub fn get_sales_stats(db: &Option<DbHandle>, input: GetSalesStatsInput) -> String {
    let Some(handle) = db else { return db_unavailable() };
    let guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return lock_err(),
    };
    match crate::db::tracking::get_sales_stats(&guard.conn, input.platform.as_deref(), input.category.as_deref()) {
        Ok(stats) => serde_json::to_string(&stats).unwrap_or_else(|e| e.to_string()),
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    }
}
```

Then add to `src/db/tracking.rs`:

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct SalesStats {
    pub total_tracked: i64,
    pub active_count: i64,
    pub sold_count: i64,
    pub removed_count: i64,
    pub avg_days_listed: Option<f64>,
    pub price_drop_count: i64,
    pub avg_price_drop_pct: Option<f64>,
    pub avg_sold_price: Option<f64>,
}

pub fn get_sales_stats(conn: &Connection, platform: Option<&str>, category: Option<&str>) -> anyhow::Result<SalesStats> {
    // Base counts
    let (where_clause, param_values) = build_stats_filter(platform, category);

    let sql = format!(
        "SELECT
           COUNT(*) as total,
           SUM(CASE WHEN t.status='active' THEN 1 ELSE 0 END),
           SUM(CASE WHEN t.status='sold' THEN 1 ELSE 0 END),
           SUM(CASE WHEN t.status='removed' THEN 1 ELSE 0 END)
         FROM tracked_items t
         JOIN listings l ON t.listing_id = l.id
         {where_clause}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let (total, active, sold, removed): (i64, i64, i64, i64) =
        stmt.query_row(rusqlite::params_from_iter(&param_values), |r| {
            Ok((r.get(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))
        })?;

    // Avg days listed for sold items (status_changed_at - first_seen_at)
    let sql2 = format!(
        "SELECT AVG(julianday(t.status_changed_at) - julianday(t.first_seen_at))
         FROM tracked_items t
         JOIN listings l ON t.listing_id = l.id
         WHERE t.status = 'sold' AND t.status_changed_at IS NOT NULL
         {}", if where_clause.is_empty() { String::new() } else { format!("AND {}", &where_clause[6..]) }
    );
    let mut stmt2 = conn.prepare(&sql2)?;
    let avg_days: Option<f64> = stmt2.query_row(rusqlite::params_from_iter(&param_values), |r| r.get(0))?;

    // Price drop: items where current_price < first_seen_price
    let sql3 = format!(
        "SELECT COUNT(*), AVG((t.first_seen_price - t.current_price) / t.first_seen_price * 100)
         FROM tracked_items t
         JOIN listings l ON t.listing_id = l.id
         WHERE t.first_seen_price IS NOT NULL AND t.current_price IS NOT NULL
           AND t.current_price < t.first_seen_price
         {}", if where_clause.is_empty() { String::new() } else { format!("AND {}", &where_clause[6..]) }
    );
    let mut stmt3 = conn.prepare(&sql3)?;
    let (drop_count, avg_drop_pct): (i64, Option<f64>) =
        stmt3.query_row(rusqlite::params_from_iter(&param_values), |r| Ok((r.get(0)?, r.get(1)?)))?;

    // Avg sold price
    let sql4 = format!(
        "SELECT AVG(t.sold_price)
         FROM tracked_items t
         JOIN listings l ON t.listing_id = l.id
         WHERE t.status = 'sold' AND t.sold_price IS NOT NULL
         {}", if where_clause.is_empty() { String::new() } else { format!("AND {}", &where_clause[6..]) }
    );
    let mut stmt4 = conn.prepare(&sql4)?;
    let avg_sold: Option<f64> = stmt4.query_row(rusqlite::params_from_iter(&param_values), |r| r.get(0))?;

    Ok(SalesStats {
        total_tracked: total,
        active_count: active,
        sold_count: sold,
        removed_count: removed,
        avg_days_listed: avg_days,
        price_drop_count: drop_count,
        avg_price_drop_pct: avg_drop_pct,
        avg_sold_price: avg_sold,
    })
}

fn build_stats_filter(platform: Option<&str>, category: Option<&str>) -> (String, Vec<String>) {
    let mut clauses = Vec::new();
    let mut values = Vec::new();
    if let Some(p) = platform {
        clauses.push("t.platform = ?".to_string());
        values.push(p.to_string());
    }
    if let Some(c) = category {
        clauses.push("l.category = ?".to_string());
        values.push(c.to_string());
    }
    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    (where_clause, values)
}
```

## 5. Update src/tools/search.rs (KslMcpServer)

Add imports at top:

```rust
use crate::tools::searches::{SaveSearchInput, DeleteSavedSearchInput, RunSavedSearchInput};
use crate::tools::tracking::GetSalesStatsInput;
use crate::db::searches::SavedSearchParams;
```

Add these tool methods inside the `#[tool(tool_box)] impl KslMcpServer` block:

```rust
/// Save a search for later re-use
#[tool(description = "Save search parameters for later re-use.")]
fn save_search(&self, #[tool(aggr)] input: SaveSearchInput) -> String {
    crate::tools::searches::save_search(&self.db, input)
}

/// List all saved searches
#[tool(description = "List all saved searches.")]
fn list_saved_searches(&self) -> String {
    crate::tools::searches::list_saved_searches(&self.db)
}

/// Delete a saved search
#[tool(description = "Delete a saved search by ID.")]
fn delete_saved_search(&self, #[tool(aggr)] input: DeleteSavedSearchInput) -> String {
    crate::tools::searches::delete_saved_search(&self.db, input)
}

/// Run a saved search
#[tool(description = "Run a previously saved search and return results.")]
async fn run_saved_search(&self, #[tool(aggr)] input: RunSavedSearchInput) -> String {
    let (_, params) = match crate::tools::searches::get_saved_search_for_run(&self.db, &input) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match params {
        SavedSearchParams::Classifieds(p) => {
            match self.classifieds_client.search_classifieds(&p).await {
                Ok(r) => serde_json::to_string(&r).unwrap_or_else(|e| e.to_string()),
                Err(e) => format!("Error: {e}"),
            }
        }
        SavedSearchParams::Cars(p) => {
            match self.cars_client.search_cars(&p).await {
                Ok(r) => serde_json::to_string(&r).unwrap_or_else(|e| e.to_string()),
                Err(e) => format!("Error: {e}"),
            }
        }
    }
}

/// Get aggregate sales statistics from tracked data
#[tool(description = "Get aggregate statistics from tracked listings: avg days listed, price drops, sold averages.")]
fn get_sales_stats(&self, #[tool(aggr)] input: GetSalesStatsInput) -> String {
    crate::tools::tracking::get_sales_stats(&self.db, input)
}
```

## 6. Summary of Changes

| File | Action |
|------|--------|
| `src/db/mod.rs` | Add `pub mod searches;`, add `SCHEMA_V2`, update `migrate()` |
| `src/db/searches.rs` | **New** — CRUD for saved_searches table |
| `src/db/tracking.rs` | Add `SalesStats` struct and `get_sales_stats()` + `build_stats_filter()` |
| `src/tools/mod.rs` | Add `pub mod searches;` |
| `src/tools/searches.rs` | **New** — tool input structs + functions |
| `src/tools/tracking.rs` | Add `GetSalesStatsInput` struct + `get_sales_stats()` wrapper |
| `src/tools/search.rs` | Add 5 new tool methods + imports |

## Key Design Decisions

- **JSON storage**: `parameters` column is `TEXT` containing JSON. The `SavedSearchParams` enum uses `#[serde(tag = "platform")]` for tagged deserialization — the platform field in JSON determines which variant to deserialize into.
- **No raw interpolation**: All SQL uses `?` placeholders via `params![]` or `params_from_iter`. The `build_stats_filter` function constructs WHERE clauses with positional params, never string-interpolated values.
- **run_saved_search** deserializes params into typed `ClassifiedsSearchParams`/`CarsSearchParams` and calls the existing client methods directly — same code path as manual search.
- **Schema version bump**: v1 → v2. Migration is additive (new table only), safe for existing data.
