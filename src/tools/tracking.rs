use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::{
    db::tracking as db,
    types::{Listing, Platform},
};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct TrackItemInput {
    /// Listing ID to track
    pub listing_id: String,
    /// Platform: "classifieds" or "cars"
    pub platform: String,
    /// Optional notes
    pub notes: Option<String>,
    /// Listing title (required if listing not yet in DB)
    pub title: String,
    /// Listing URL
    pub url: String,
    /// Current price
    pub price: Option<f64>,
    /// City
    pub city: Option<String>,
    /// State
    pub state: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UntrackItemInput {
    /// Listing ID to untrack
    pub listing_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetPriceHistoryInput {
    /// Listing ID
    pub listing_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MarkAsSoldInput {
    /// Listing ID
    pub listing_id: String,
    /// Optional sold price (uses current price if omitted)
    pub sold_price: Option<f64>,
}

pub type DbHandle = Arc<Mutex<crate::db::Db>>;

fn db_unavailable() -> String {
    r#"{"error":"database unavailable (degraded mode)"}"#.to_string()
}

fn lock_err() -> String {
    r#"{"error":"database lock poisoned"}"#.to_string()
}

pub fn track_item(db: &Option<DbHandle>, input: TrackItemInput) -> String {
    let Some(handle) = db else { return db_unavailable() };
    let mut guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return lock_err(),
    };
    let listing = Listing {
        id: input.listing_id,
        title: input.title,
        price: input.price,
        city: input.city,
        state: input.state,
        url: input.url,
        image_url: None,
        category: None,
        favorites_count: None,
        platform: Platform::from_str(&input.platform),
    };
    match db::track_item(&mut guard.conn, &listing, input.notes.as_deref()) {
        Ok(row) => serde_json::to_string(&row).unwrap_or_else(|e| e.to_string()),
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    }
}

pub fn untrack_item(db: &Option<DbHandle>, input: UntrackItemInput) -> String {
    let Some(handle) = db else { return db_unavailable() };
    let guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return lock_err(),
    };
    match db::untrack_item(&guard.conn, &input.listing_id) {
        Ok(removed) => format!(r#"{{"removed":{removed}}}"#),
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    }
}

pub fn list_tracked_items(db: &Option<DbHandle>) -> String {
    let Some(handle) = db else { return db_unavailable() };
    let guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return lock_err(),
    };
    match db::list_tracked_items(&guard.conn) {
        Ok(rows) => serde_json::to_string(&rows).unwrap_or_else(|e| e.to_string()),
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    }
}

pub fn get_price_history(db: &Option<DbHandle>, input: GetPriceHistoryInput) -> String {
    let Some(handle) = db else { return db_unavailable() };
    let guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return lock_err(),
    };
    match db::get_price_history(&guard.conn, &input.listing_id) {
        Ok(rows) => serde_json::to_string(&rows).unwrap_or_else(|e| e.to_string()),
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    }
}

pub fn mark_as_sold(db: &Option<DbHandle>, input: MarkAsSoldInput) -> String {
    let Some(handle) = db else { return db_unavailable() };
    let mut guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return lock_err(),
    };
    match db::mark_as_sold(&mut guard.conn, &input.listing_id, input.sold_price) {
        Ok(()) => r#"{"ok":true}"#.to_string(),
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetSalesStatsInput {
    /// Optional platform filter: "classifieds" or "cars"
    pub platform: Option<String>,
    /// Optional category filter
    pub category: Option<String>,
}

pub fn get_sales_stats(db: &Option<DbHandle>, input: GetSalesStatsInput) -> String {
    let Some(handle) = db else {
        return db_unavailable();
    };
    let guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return lock_err(),
    };
    match crate::db::tracking::get_sales_stats(
        &guard.conn,
        input.platform.as_deref(),
        input.category.as_deref(),
    ) {
        Ok(stats) => serde_json::to_string(&stats).unwrap_or_else(|e| e.to_string()),
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    }
}