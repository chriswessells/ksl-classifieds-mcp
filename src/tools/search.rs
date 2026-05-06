use rmcp::{ServerHandler, model::ServerInfo, schemars, tool};
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
    types::{CarsSearchParams, ClassifiedsSearchParams, SortOrder},
};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SearchInput {
    /// Keyword to search for
    pub keyword: Option<String>,
    /// Category name (e.g. "Cycling", "Electronics")
    pub category: Option<String>,
    /// Minimum price in dollars
    pub price_from: Option<u32>,
    /// Maximum price in dollars
    pub price_to: Option<u32>,
    /// ZIP code for radius search
    pub zip: Option<String>,
    /// Radius in miles
    pub miles: Option<u32>,
    /// Sort order: Newest, Oldest, PriceLow, PriceHigh
    pub sort: Option<String>,
    /// Page number (0-indexed)
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
    /// ZIP code for radius search
    pub zip: Option<String>,
    /// Radius in miles
    pub miles: Option<u32>,
    /// Keyword search
    pub keyword: Option<String>,
    /// Page number (1-indexed)
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
    /// Search KSL Classifieds listings
    #[tool(description = "Search KSL Classifieds listings by keyword, category, price, location, and more.")]
    async fn search_classifieds(&self, #[tool(aggr)] input: SearchInput) -> String {
        let sort = input.sort.as_deref().and_then(|s| match s {
            "Newest" => Some(SortOrder::Newest),
            "Oldest" => Some(SortOrder::Oldest),
            "PriceLow" => Some(SortOrder::PriceLow),
            "PriceHigh" => Some(SortOrder::PriceHigh),
            _ => None,
        });

        let params = ClassifiedsSearchParams {
            keyword: input.keyword,
            category: input.category,
            price_from: input.price_from,
            price_to: input.price_to,
            zip: input.zip,
            miles: input.miles,
            sort,
            page: input.page,
            ..Default::default()
        };

        match self.classifieds_client.search_classifieds(&params).await {
            Ok(results) => serde_json::to_string(&results).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("Error: {}", e),
        }
    }

    /// List all available KSL Classifieds categories
    #[tool(description = "List all available KSL Classifieds categories with their IDs and names.")]
    fn list_categories(&self) -> String {
        let cats = categories::all_categories();
        serde_json::to_string(cats).unwrap_or_else(|e| e.to_string())
    }

    /// Search KSL Cars listings
    #[tool(description = "Search KSL Cars listings by make, model, year, price, mileage, and location.")]
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

    /// Get full details for a KSL Classifieds listing
    #[tool(description = "Get full details for a KSL Classifieds listing including description, photos, and seller info.")]
    async fn get_listing(&self, #[tool(aggr)] input: GetListingInput) -> String {
        match self.classifieds_client.get_listing_detail(&input.id).await {
            Ok(detail) => serde_json::to_string(&detail).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("Error: {}", e),
        }
    }

    /// Track a listing for price monitoring
    #[tool(description = "Add a listing to your watch list for price tracking.")]
    fn track_item(&self, #[tool(aggr)] input: TrackItemInput) -> String {
        crate::tools::tracking::track_item(&self.db, input)
    }

    /// Remove a listing from tracking
    #[tool(description = "Remove a listing from your watch list.")]
    fn untrack_item(&self, #[tool(aggr)] input: UntrackItemInput) -> String {
        crate::tools::tracking::untrack_item(&self.db, input)
    }

    /// List all tracked items
    #[tool(description = "List all tracked listings with current price and status.")]
    fn list_tracked_items(&self) -> String {
        crate::tools::tracking::list_tracked_items(&self.db)
    }

    /// Get price history for a tracked listing
    #[tool(description = "Get price snapshot history for a tracked listing.")]
    fn get_price_history(&self, #[tool(aggr)] input: GetPriceHistoryInput) -> String {
        crate::tools::tracking::get_price_history(&self.db, input)
    }

    /// Mark a tracked listing as sold
    #[tool(description = "Manually mark a tracked listing as sold.")]
    fn mark_as_sold(&self, #[tool(aggr)] input: MarkAsSoldInput) -> String {
        crate::tools::tracking::mark_as_sold(&self.db, input)
    }

    /// Browse search results in an interactive HTML report
    #[tool(description = "Search KSL Classifieds and open an interactive HTML report in the browser with photos and checkboxes for selecting items.")]
    async fn browse_search_results(&self, #[tool(aggr)] input: BrowseSearchResultsInput) -> String {
        report_tools::browse_search_results(&self.classifieds_client, &self.report_server, input).await
    }

    /// Check for pending user selections from a browse report
    #[tool(description = "Check if the user has submitted item selections from a browse_search_results report.")]
    fn get_pending_selections(&self) -> String {
        report_tools::get_pending_selections(&self.report_server)
    }

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

    /// Delete a saved search by ID
    #[tool(description = "Delete a saved search by ID.")]
    fn delete_saved_search(&self, #[tool(aggr)] input: DeleteSavedSearchInput) -> String {
        crate::tools::searches::delete_saved_search(&self.db, input)
    }

    /// Run a previously saved search
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
}

#[tool(tool_box)]
impl ServerHandler for KslMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Search KSL Classifieds and KSL Cars listings, browse categories, and get listing details.".into(),
            ),
            ..Default::default()
        }
    }
}
