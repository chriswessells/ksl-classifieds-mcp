use anyhow::anyhow;
use chrono::Utc;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::types::{Listing, TrackingStatus};

#[derive(Debug, Serialize, Deserialize)]
pub struct TrackedItemRow {
    pub id: i64,
    pub listing_id: String,
    pub platform: String,
    pub notes: Option<String>,
    pub first_seen_price: Option<f64>,
    pub current_price: Option<f64>,
    pub first_seen_at: String,
    pub last_checked_at: String,
    pub status: String,
    pub status_changed_at: Option<String>,
    pub sold_price: Option<f64>,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PriceSnapshot {
    pub price: f64,
    pub recorded_at: String,
}

pub fn track_item(
    conn: &mut Connection,
    listing: &Listing,
    notes: Option<&str>,
) -> anyhow::Result<TrackedItemRow> {
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();

    tx.execute(
        "INSERT INTO listings (id, platform, title, price, url, city, state, first_seen_at, last_fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, COALESCE((SELECT first_seen_at FROM listings WHERE id=?1), ?8), ?9)
         ON CONFLICT(id) DO UPDATE SET last_fetched_at=excluded.last_fetched_at, price=excluded.price",
        params![
            listing.id,
            listing.platform.to_str(),
            listing.title,
            listing.price,
            listing.url,
            listing.city,
            listing.state,
            &now,
            &now
        ],
    )?;

    // Idempotency: return existing row if already tracked
    if let Some(row) = get_tracked_item_by_listing_tx(&tx, &listing.id)? {
        tx.commit()?;
        return Ok(row);
    }

    tx.execute(
        "INSERT INTO tracked_items (listing_id, platform, notes, first_seen_price, current_price, first_seen_at, last_checked_at, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active')",
        params![
            listing.id,
            listing.platform.to_str(),
            notes,
            listing.price,
            listing.price,
            &now,
            &now
        ],
    )?;

    insert_snapshot_if_new(&tx, &listing.id, listing.price, &now)?;

    tx.commit()?;
    get_tracked_item_by_listing(conn, &listing.id)?.ok_or_else(|| anyhow!("insert failed"))
}

pub fn untrack_item(conn: &Connection, listing_id: &str) -> anyhow::Result<bool> {
    let deleted =
        conn.execute("DELETE FROM tracked_items WHERE listing_id = ?1", params![listing_id])?;
    Ok(deleted > 0)
}

pub fn list_tracked_items(conn: &Connection) -> anyhow::Result<Vec<TrackedItemRow>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.listing_id, t.platform, t.notes, t.first_seen_price, t.current_price,
                t.first_seen_at, t.last_checked_at, t.status, t.status_changed_at, t.sold_price,
                l.title, l.url
         FROM tracked_items t
         JOIN listings l ON t.listing_id = l.id
         ORDER BY t.first_seen_at DESC",
    )?;
    let rows = stmt.query_map([], map_row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_price_history(
    conn: &Connection,
    listing_id: &str,
) -> anyhow::Result<Vec<PriceSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT price, recorded_at FROM price_snapshots WHERE listing_id = ?1 ORDER BY recorded_at ASC",
    )?;
    let rows = stmt
        .query_map(params![listing_id], |r| {
            Ok(PriceSnapshot { price: r.get(0)?, recorded_at: r.get(1)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn mark_as_sold(
    conn: &mut Connection,
    listing_id: &str,
    sold_price: Option<f64>,
) -> anyhow::Result<()> {
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();

    let price = sold_price.or_else(|| get_current_price(&tx, listing_id));

    let updated = tx.execute(
        "UPDATE tracked_items SET status = 'sold', status_changed_at = ?1, sold_price = ?2
         WHERE listing_id = ?3 AND status = 'active'",
        params![&now, price, listing_id],
    )?;

    if updated > 0 {
        if let Some(p) = price {
            insert_snapshot_if_new(&tx, listing_id, Some(p), &now)?;
        }
        tracing::info!(
            listing_id = listing_id,
            old_status = TrackingStatus::Active.to_str(),
            new_status = TrackingStatus::Sold.to_str(),
            trigger = "manual",
            "status transition"
        );
    }

    tx.commit()?;
    Ok(())
}

// --- helpers ---

fn get_tracked_item_by_listing(
    conn: &Connection,
    listing_id: &str,
) -> anyhow::Result<Option<TrackedItemRow>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.listing_id, t.platform, t.notes, t.first_seen_price, t.current_price,
                t.first_seen_at, t.last_checked_at, t.status, t.status_changed_at, t.sold_price,
                l.title, l.url
         FROM tracked_items t
         JOIN listings l ON t.listing_id = l.id
         WHERE t.listing_id = ?1",
    )?;
    let mut rows = stmt.query_map(params![listing_id], map_row)?;
    Ok(rows.next().transpose()?)
}

fn get_tracked_item_by_listing_tx(
    tx: &rusqlite::Transaction<'_>,
    listing_id: &str,
) -> anyhow::Result<Option<TrackedItemRow>> {
    let mut stmt = tx.prepare(
        "SELECT t.id, t.listing_id, t.platform, t.notes, t.first_seen_price, t.current_price,
                t.first_seen_at, t.last_checked_at, t.status, t.status_changed_at, t.sold_price,
                l.title, l.url
         FROM tracked_items t
         JOIN listings l ON t.listing_id = l.id
         WHERE t.listing_id = ?1",
    )?;
    let mut rows = stmt.query_map(params![listing_id], map_row)?;
    Ok(rows.next().transpose()?)
}

fn get_current_price(conn: &Connection, listing_id: &str) -> Option<f64> {
    conn.query_row(
        "SELECT current_price FROM tracked_items WHERE listing_id = ?1",
        params![listing_id],
        |r| r.get(0),
    )
    .ok()
    .flatten()
}

fn insert_snapshot_if_new(
    conn: &Connection,
    listing_id: &str,
    price: Option<f64>,
    now: &str,
) -> anyhow::Result<()> {
    let Some(p) = price else { return Ok(()) };
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM price_snapshots WHERE listing_id = ?1 AND price = ?2
             AND recorded_at > datetime(?3, '-60 seconds') LIMIT 1",
            params![listing_id, p, now],
            |_| Ok(true),
        )
        .unwrap_or(false);
    if !exists {
        conn.execute(
            "INSERT INTO price_snapshots (listing_id, price, recorded_at) VALUES (?1, ?2, ?3)",
            params![listing_id, p, now],
        )?;
    }
    Ok(())
}

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TrackedItemRow> {
    Ok(TrackedItemRow {
        id: r.get(0)?,
        listing_id: r.get(1)?,
        platform: r.get(2)?,
        notes: r.get(3)?,
        first_seen_price: r.get(4)?,
        current_price: r.get(5)?,
        first_seen_at: r.get(6)?,
        last_checked_at: r.get(7)?,
        status: r.get(8)?,
        status_changed_at: r.get(9)?,
        sold_price: r.get(10)?,
        title: r.get(11)?,
        url: r.get(12)?,
    })
}

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

pub fn get_sales_stats(
    conn: &Connection,
    platform: Option<&str>,
    category: Option<&str>,
) -> anyhow::Result<SalesStats> {
    let mut clauses: Vec<&str> = Vec::new();
    let mut values: Vec<String> = Vec::new();
    if let Some(p) = platform {
        clauses.push("t.platform = ?");
        values.push(p.to_string());
    }
    if let Some(c) = category {
        clauses.push("l.category = ?");
        values.push(c.to_string());
    }
    let base_where = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    let and_filter = if clauses.is_empty() {
        String::new()
    } else {
        format!("AND {}", clauses.join(" AND "))
    };

    let sql = format!(
        "SELECT COUNT(*),
                SUM(CASE WHEN t.status='active' THEN 1 ELSE 0 END),
                SUM(CASE WHEN t.status='sold' THEN 1 ELSE 0 END),
                SUM(CASE WHEN t.status='removed' THEN 1 ELSE 0 END)
         FROM tracked_items t JOIN listings l ON t.listing_id = l.id {base_where}"
    );
    let (total, active, sold, removed): (i64, i64, i64, i64) = conn
        .prepare(&sql)?
        .query_row(rusqlite::params_from_iter(&values), |r| {
            Ok((r.get(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))
        })?;

    let sql2 = format!(
        "SELECT AVG(julianday(t.status_changed_at) - julianday(t.first_seen_at))
         FROM tracked_items t JOIN listings l ON t.listing_id = l.id
         WHERE t.status = 'sold' AND t.status_changed_at IS NOT NULL {and_filter}"
    );
    let avg_days: Option<f64> = conn
        .prepare(&sql2)?
        .query_row(rusqlite::params_from_iter(&values), |r| r.get(0))?;

    let sql3 = format!(
        "SELECT COUNT(*), AVG((t.first_seen_price - t.current_price) / t.first_seen_price * 100)
         FROM tracked_items t JOIN listings l ON t.listing_id = l.id
         WHERE t.first_seen_price IS NOT NULL AND t.current_price IS NOT NULL
           AND t.current_price < t.first_seen_price {and_filter}"
    );
    let (drop_count, avg_drop_pct): (i64, Option<f64>) = conn
        .prepare(&sql3)?
        .query_row(rusqlite::params_from_iter(&values), |r| Ok((r.get(0)?, r.get(1)?)))?;

    let sql4 = format!(
        "SELECT AVG(t.sold_price)
         FROM tracked_items t JOIN listings l ON t.listing_id = l.id
         WHERE t.status = 'sold' AND t.sold_price IS NOT NULL {and_filter}"
    );
    let avg_sold: Option<f64> = conn
        .prepare(&sql4)?
        .query_row(rusqlite::params_from_iter(&values), |r| r.get(0))?;

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