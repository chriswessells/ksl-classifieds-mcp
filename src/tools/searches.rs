use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::db::searches::{self as db, SavedSearchParams};
use crate::tools::tracking::DbHandle;
use crate::types::{CarsSearchParams, ClassifiedsSearchParams, SortOrder};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SaveSearchInput {
    /// Name for this saved search
    pub name: String,
    /// Platform: "classifieds" or "cars"
    pub platform: String,
    pub keyword: Option<String>,
    pub category: Option<String>,
    pub price_from: Option<u32>,
    pub price_to: Option<u32>,
    pub zip: Option<String>,
    pub miles: Option<u32>,
    pub sort: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub year_from: Option<u32>,
    pub year_to: Option<u32>,
    pub mileage_from: Option<u32>,
    pub mileage_to: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DeleteSavedSearchInput {
    pub id: i64,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunSavedSearchInput {
    pub id: i64,
}

fn db_unavailable() -> String {
    r#"{"error":"database unavailable (degraded mode)"}"#.to_string()
}

fn lock_err() -> String {
    r#"{"error":"database lock poisoned"}"#.to_string()
}

pub fn save_search(db_handle: &Option<DbHandle>, input: SaveSearchInput) -> String {
    let Some(handle) = db_handle else {
        return db_unavailable();
    };
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
    let Some(handle) = db_handle else {
        return db_unavailable();
    };
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
    let Some(handle) = db_handle else {
        return db_unavailable();
    };
    let guard = match handle.lock() {
        Ok(g) => g,
        Err(_) => return lock_err(),
    };
    match db::delete_saved_search(&guard.conn, input.id) {
        Ok(removed) => format!(r#"{{"removed":{removed}}}"#),
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    }
}

pub fn get_saved_search_for_run(
    db_handle: &Option<DbHandle>,
    input: &RunSavedSearchInput,
) -> Result<(db::SavedSearchRow, SavedSearchParams), String> {
    let Some(handle) = db_handle else {
        return Err(db_unavailable());
    };
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
        "Newest" => Some(SortOrder::Newest),
        "Oldest" => Some(SortOrder::Oldest),
        "PriceLow" => Some(SortOrder::PriceLow),
        "PriceHigh" => Some(SortOrder::PriceHigh),
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
