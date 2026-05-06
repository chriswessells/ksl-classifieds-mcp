pub mod cars;
pub mod classifieds;
pub mod rate_limiter;

use crate::error::Result;
use crate::types::{ClassifiedsSearchParams, ListingDetail, SearchResults};

pub trait KslClient: Send + Sync {
    async fn search_classifieds(&self, params: &ClassifiedsSearchParams) -> Result<SearchResults>;
    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail>;
}
