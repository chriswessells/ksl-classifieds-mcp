use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    client::{KslClient, classifieds::ClassifiedsClient},
    report::ReportServer,
    types::{ClassifiedsSearchParams, SortParam},
};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BrowseSearchResultsInput {
    /// Keyword to search for
    pub keyword: Option<String>,
    /// Category name (use ksl_list_categories to see options)
    pub category: Option<String>,
    /// Minimum price in dollars
    pub price_from: Option<u32>,
    /// Maximum price in dollars
    pub price_to: Option<u32>,
    /// ZIP code for location-based search
    pub zip: Option<String>,
    /// Radius in miles from ZIP code
    pub miles: Option<u32>,
    /// Sort order for results
    pub sort: Option<SortParam>,
    /// Page number (1-indexed, default 1)
    pub page: Option<u32>,
}

pub async fn browse_search_results(
    client: &ClassifiedsClient,
    report_server: &ReportServer,
    input: BrowseSearchResultsInput,
) -> String {
    if let Err(e) = report_server.ensure_started().await {
        return format!(r#"{{"error":"Report server failed to start: {e}"}}"#);
    }

    let page_0 = input.page.map(|p| p.saturating_sub(1));
    let params = ClassifiedsSearchParams {
        keyword: input.keyword,
        category: input.category,
        price_from: input.price_from,
        price_to: input.price_to,
        zip: input.zip,
        miles: input.miles,
        sort: input.sort.map(|s| s.to_sort_order()),
        page: page_0,
        ..Default::default()
    };

    let results = match client.search_classifieds(&params).await {
        Ok(r) => r,
        Err(e) => return format!(r#"{{"error":"Search failed: {e}"}}"#),
    };

    if results.listings.is_empty() {
        return r#"{"error":"No results found."}"#.to_string();
    }

    let (url, report_id) = report_server.register_report(&results.listings);

    if let Err(e) = open_browser(&url) {
        return format!(r#"{{"error":"Failed to open browser: {e}","url":"{url}"}}"#);
    }

    format!(
        r#"{{"ok":true,"report_id":"{report_id}","url":"{url}","listing_count":{count},"message":"Report opened in browser. Select items and submit, then call get_pending_selections."}}"#,
        count = results.listings.len(),
    )
}

pub fn get_pending_selections(report_server: &ReportServer) -> String {
    match report_server.take_pending_selections() {
        Some((report_id, selections)) => serde_json::json!({
            "report_id": report_id.to_string(),
            "selected_listing_ids": selections,
            "count": selections.len(),
        })
        .to_string(),
        None => r#"{"pending":false,"message":"No selections submitted yet."}"#.to_string(),
    }
}

fn open_browser(url: &str) -> Result<(), String> {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "linux") {
        "xdg-open"
    } else {
        return Err("Unsupported platform".to_string());
    };
    std::process::Command::new(cmd)
        .arg(url)
        .spawn()
        .map_err(|e| format!("{cmd} failed: {e}"))?;
    Ok(())
}
