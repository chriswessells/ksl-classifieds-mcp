use rmcp::{ServerHandler, model::{ServerCapabilities, ServerInfo, ToolsCapability}, schemars, tool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    categories,
    client::{KslClient, cars::CarsClient, classifieds::ClassifiedsClient},
    db::searches::SavedSearchParams,
    report::ReportServer,
    tools::{
        report::{self as report_tools, BrowseSearchResultsInput},
        searches::{DeleteSavedSearchInput, RunSavedSearchInput, SaveSearchInput},
        tracking::{GetPriceHistoryInput, GetSalesStatsInput, MarkAsSoldInput, TrackItemInput, UntrackItemInput},
    },
    types::{CarsSearchParams, ClassifiedsSearchParams, SortParam},
};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SearchInput {
    /// Keyword to search for
    pub keyword: Option<String>,
    /// Category name (e.g. "Cycling", "Electronics"). Use ksl_list_categories to see all options.
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

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SearchCarsInput {
    /// Car make (e.g. "Toyota", "Ford;Honda" for multiple)
    pub make: Option<String>,
    /// Car model (e.g. "Camry")
    pub model: Option<String>,
    /// Minimum year
    pub year_from: Option<u32>,
    /// Maximum year
    pub year_to: Option<u32>,
    /// Minimum price in dollars
    pub price_from: Option<u32>,
    /// Maximum price in dollars
    pub price_to: Option<u32>,
    /// Minimum mileage
    pub mileage_from: Option<u32>,
    /// Maximum mileage
    pub mileage_to: Option<u32>,
    /// ZIP code for location-based search
    pub zip: Option<String>,
    /// Radius in miles from ZIP code
    pub miles: Option<u32>,
    /// Keyword search
    pub keyword: Option<String>,
    /// Page number (1-indexed, default 1)
    pub page: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetListingInput {
    /// The listing ID (numeric string from the listing URL)
    pub id: String,
}

#[derive(Clone)]
pub struct KslMcpServer {
    classifieds_client: ClassifiedsClient,
    cars_client: CarsClient,
    db: Option<crate::tools::tracking::DbHandle>,
    report_server: ReportServer,
}

impl KslMcpServer {
    pub fn new(
        classifieds_client: ClassifiedsClient,
        cars_client: CarsClient,
        db: Option<crate::db::Db>,
        report_server: ReportServer,
    ) -> Self {
        let db = db.map(|d| std::sync::Arc::new(std::sync::Mutex::new(d)));
        Self { classifieds_client, cars_client, db, report_server }
    }
}

#[tool(tool_box)]
impl KslMcpServer {
    #[tool(name = "ksl_search_classifieds", description = "Search KSL Classifieds listings by keyword, category, price, location. Returns listing IDs, titles, prices, and URLs.")]
    async fn search_classifieds(&self, #[tool(aggr)] input: SearchInput) -> String {
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

        match self.classifieds_client.search_classifieds(&params).await {
            Ok(results) => serde_json::to_string(&results).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(name = "ksl_list_categories", description = "List all available KSL Classifieds categories with their IDs and names.")]
    fn list_categories(&self) -> String {
        let cats = categories::all_categories();
        serde_json::to_string(cats).unwrap_or_else(|e| e.to_string())
    }

    #[tool(name = "ksl_search_cars", description = "Search KSL Cars listings by make, model, year, price, mileage, and location.")]
    async fn search_cars(&self, #[tool(aggr)] input: SearchCarsInput) -> String {
        let params = CarsSearchParams {
            keyword: input.keyword,
            make: input.make,
            model: input.model,
            year_from: input.year_from,
            year_to: input.year_to,
            price_from: input.price_from,
            price_to: input.price_to,
            mileage_from: input.mileage_from,
            mileage_to: input.mileage_to,
            zip: input.zip,
            miles: input.miles,
            page: input.page,
            ..Default::default()
        };

        match self.cars_client.search_cars(&params).await {
            Ok(results) => serde_json::to_string(&results).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(name = "ksl_get_listing", description = "Get full details for a KSL Classifieds listing including description, photos, and seller info.")]
    async fn get_listing(&self, #[tool(aggr)] input: GetListingInput) -> String {
        match self.classifieds_client.get_listing_detail(&input.id).await {
            Ok(detail) => serde_json::to_string(&detail).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(name = "ksl_track_item", description = "Track a listing for price monitoring. Provide listing_id and platform; details are auto-fetched from KSL.")]
    async fn track_item(&self, #[tool(aggr)] input: TrackItemInput) -> String {
        crate::tools::tracking::track_item(&self.classifieds_client, &self.cars_client, &self.db, input).await
    }

    #[tool(name = "ksl_untrack_item", description = "Remove a listing from your watch list.")]
    fn untrack_item(&self, #[tool(aggr)] input: UntrackItemInput) -> String {
        crate::tools::tracking::untrack_item(&self.db, input)
    }

    #[tool(name = "ksl_list_tracked_items", description = "List all tracked listings with current price and status.")]
    fn list_tracked_items(&self) -> String {
        crate::tools::tracking::list_tracked_items(&self.db)
    }

    #[tool(name = "ksl_get_price_history", description = "Get price snapshot history for a tracked listing.")]
    fn get_price_history(&self, #[tool(aggr)] input: GetPriceHistoryInput) -> String {
        crate::tools::tracking::get_price_history(&self.db, input)
    }

    #[tool(name = "ksl_mark_as_sold", description = "Manually mark a tracked listing as sold.")]
    fn mark_as_sold(&self, #[tool(aggr)] input: MarkAsSoldInput) -> String {
        crate::tools::tracking::mark_as_sold(&self.db, input)
    }

    #[tool(name = "ksl_browse_search_results", description = "Search KSL Classifieds and open an interactive HTML report in the browser. After user submits selections, call ksl_get_pending_selections.")]
    async fn browse_search_results(&self, #[tool(aggr)] input: BrowseSearchResultsInput) -> String {
        report_tools::browse_search_results(&self.classifieds_client, &self.report_server, input).await
    }

    #[tool(name = "ksl_get_pending_selections", description = "Poll for user selections from a ksl_browse_search_results report. Returns selected listing IDs once submitted.")]
    fn get_pending_selections(&self) -> String {
        report_tools::get_pending_selections(&self.report_server)
    }

    #[tool(name = "ksl_save_search", description = "Save search parameters for later re-use with ksl_run_saved_search.")]
    fn save_search(&self, #[tool(aggr)] input: SaveSearchInput) -> String {
        crate::tools::searches::save_search(&self.db, input)
    }

    #[tool(name = "ksl_list_saved_searches", description = "List all saved searches with their IDs and parameters.")]
    fn list_saved_searches(&self) -> String {
        crate::tools::searches::list_saved_searches(&self.db)
    }

    #[tool(name = "ksl_delete_saved_search", description = "Delete a saved search by ID.")]
    fn delete_saved_search(&self, #[tool(aggr)] input: DeleteSavedSearchInput) -> String {
        crate::tools::searches::delete_saved_search(&self.db, input)
    }

    #[tool(name = "ksl_run_saved_search", description = "Run a previously saved search by ID and return fresh results.")]
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

    #[tool(name = "ksl_get_sales_stats", description = "Get aggregate statistics from tracked listings: avg days listed, price drops, sold averages.")]
    fn get_sales_stats(&self, #[tool(aggr)] input: GetSalesStatsInput) -> String {
        crate::tools::tracking::get_sales_stats(&self.db, input)
    }
}

#[tool(tool_box)]
impl ServerHandler for KslMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability { list_changed: None }),
                ..Default::default()
            },
            instructions: Some(
                "KSL Classifieds & Cars MCP server for Utah marketplace searches.\n\n\
                 SEARCH: Use ksl_search_classifieds or ksl_search_cars to find listings. Use ksl_list_categories to discover valid category names.\n\
                 DETAILS: Use ksl_get_listing to get full listing info (description, photos, seller).\n\
                 BROWSE: Use ksl_browse_search_results to open visual results in browser, then ksl_get_pending_selections to retrieve user picks.\n\
                 TRACK: Use ksl_track_item with a listing_id + platform to monitor prices. Use ksl_list_tracked_items, ksl_get_price_history, ksl_mark_as_sold.\n\
                 SAVED SEARCHES: Use ksl_save_search / ksl_run_saved_search to persist and re-run queries.\n\
                 STATS: Use ksl_get_sales_stats for market analytics on tracked data.".into(),
            ),
            ..Default::default()
        }
    }
}
