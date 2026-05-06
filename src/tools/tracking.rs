use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::{
    client::{KslClient, cars::CarsClient, classifieds::ClassifiedsClient},
    db::tracking as db,
    types::{Listing, Platform, PlatformParam},
};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct TrackItemInput {
    /// Listing ID to track (from search results)
    pub listing_id: String,
    /// Which platform the listing is on
    pub platform: PlatformParam,
    /// Optional notes about why you're tracking this
    pub notes: Option<String>,
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
    /// Optional sold price (uses current tracked price if omitted)
    pub sold_price: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetSalesStatsInput {
    /// Filter stats to a specific platform
    pub platform: Option<PlatformParam>,
    /// Filter stats to a specific category
    pub category: Option<String>,
}

pub type DbHandle = Arc<Mutex<crate::db::Db>>;

fn db_unavailable() -> String {
    r#"{"error":"database unavailable (degraded mode)"}"#.to_string()
}

fn lock_err() -> String {
    r#"{"error":"database lock poisoned"}"#.to_string()
}

pub async fn track_item(
    classifieds_client: &ClassifiedsClient,
    cars_client: &CarsClient,
    db: &Option<DbHandle>,
    input: TrackItemInput,
) -> String {
    let Some(handle) = db else { return db_unavailable() };

    let platform = input.platform.to_platform();
    let listing = match &platform {
        Platform::Classifieds => {
            match classifieds_client.get_listing_detail(&input.listing_id).await {
                Ok(detail) => Listing {
                    id: detail.id,
                    title: detail.title,
                    price: detail.price,
                    city: detail.city,
                    state: detail.state,
                    url: detail.url,
                    image_url: None,
                    category: None,
                    favorites_count: None,
                    platform: Platform::Classifieds,
                },
                Err(e) => return format!(r#"{{"error":"Failed to fetch listing: {e}"}}"#),
            }
        }
        Platform::Cars => {
            // Cars API doesn't have a single-listing endpoint; search by ID
            let params = crate::types::CarsSearchParams {
                keyword: Some(input.listing_id.clone()),
                ..Default::default()
            };
            match cars_client.search_cars(&params).await {
                Ok(results) => {
                    match results.listings.into_iter().find(|l| l.id == input.listing_id) {
                        Some(car) => Listing {
                            id: car.id,
                            title: car.title,
                            price: car.price,
                            city: car.city,
                            state: car.state,
                            url: car.url,
                            image_url: car.photo_url,
                            category: None,
                            favorites_count: None,
                            platform: Platform::Cars,
                        },
                        None => return format!(r#"{{"error":"Car listing {} not found"}}"#, input.listing_id),
                    }
                }
                Err(e) => return format!(r#"{{"error":"Failed to fetch car listing: {e}"}}"#),
            }
        }
    };

    let mut guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return lock_err(),
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

pub fn get_sales_stats(db: &Option<DbHandle>, input: GetSalesStatsInput) -> String {
    let Some(handle) = db else { return db_unavailable() };
    let guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return lock_err(),
    };
    let platform_str = input.platform.as_ref().map(|p| match p {
        PlatformParam::Classifieds => "classifieds",
        PlatformParam::Cars => "cars",
    });
    match crate::db::tracking::get_sales_stats(&guard.conn, platform_str, input.category.as_deref()) {
        Ok(stats) => serde_json::to_string(&stats).unwrap_or_else(|e| e.to_string()),
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    }
}
