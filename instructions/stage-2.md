# Stage 2: Listing Details & Cars — Implementation Instructions

## Overview

Stage 2 adds three capabilities:
1. **`get_listing` tool** — fetch a classifieds listing detail page, parse HTML for description/photos/seller info
2. **`search_cars` tool** — search KSL Cars via POST JSON proxy API
3. **`Platform::Cars` variant** — extend the existing enum

---

## Work Items

### 1. Types — `src/types.rs`

Add `Platform::Cars` variant and new types:

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    Classifieds,
    Cars,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingDetail {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub price: Option<f64>,
    pub photos: Vec<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub seller_type: Option<String>,
    pub posted_date: Option<String>,
    pub condition: Option<String>,
    pub url: String,
    pub platform: Platform,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CarsSearchParams {
    pub keyword: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub year_from: Option<u32>,
    pub year_to: Option<u32>,
    pub price_from: Option<u32>,
    pub price_to: Option<u32>,
    pub mileage_from: Option<u32>,
    pub mileage_to: Option<u32>,
    pub zip: Option<String>,
    pub miles: Option<u32>,
    pub title_type: Option<String>,
    pub drive: Option<String>,
    pub fuel: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarListing {
    pub id: String,
    pub title: String,
    pub price: Option<f64>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub year: Option<u32>,
    pub mileage: Option<u32>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub zip: Option<String>,
    pub photo_url: Option<String>,
    pub description: Option<String>,
    pub seller_type: Option<String>,
    pub url: String,
    pub platform: Platform,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarsSearchResults {
    pub listings: Vec<CarListing>,
    pub page: u32,
    pub has_more: bool,
}
```

---

### 2. Cars Client — `src/client/cars.rs` (new file)

**API Request Format:**

```
POST https://cars.ksl.com/nextjs-api/proxy
Content-Type: application/json
Origin: https://cars.ksl.com
User-Agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15
```

**JSON body structure:**
```json
{
  "endpoint": "/classifieds/cars/search/searchByUrlParams",
  "options": {
    "method": "POST",
    "headers": {
      "Content-Type": "application/json",
      "User-Agent": "cars-node",
      "X-App-Source": "frontline",
      "X-DDM-EVENT-USER-AGENT": {},
      "X-DDM-EVENT-ACCEPT-LANGUAGE": "en-US",
      "X-MEMBER-ID": null,
      "cookie": ""
    },
    "body": [
      "make", "Ford",
      "priceFrom", "2000",
      "priceTo", "5000",
      "perPage", 24,
      "page", 1
    ]
  }
}
```

The `options.body` is a **flat array of alternating key/value pairs**. Values can be strings or numbers. Use `serde_json::Value` for the body array elements.

**Implementation:**

```rust
use serde_json::{json, Value};

const CARS_API_URL: &str = "https://cars.ksl.com/nextjs-api/proxy";

pub struct CarsClient {
    http: reqwest::Client,
    rate_limiter: RateLimiter,
}

impl CarsClient {
    pub fn new(config: &Config) -> Self { /* ... */ }

    pub async fn search_cars(&self, params: &CarsSearchParams) -> Result<CarsSearchResults> {
        self.rate_limiter.acquire().await?;
        let body = self.build_request_body(params);
        // POST to CARS_API_URL with JSON body
        // Parse response: { "data": { "items": [...] } }
    }

    fn build_request_body(&self, params: &CarsSearchParams) -> Value {
        let mut body_array: Vec<Value> = Vec::new();

        // Helper: push key then value
        if let Some(make) = &params.make {
            body_array.push(json!("make"));
            body_array.push(json!(make));
        }
        // ... repeat for all params ...

        // Always include pagination
        body_array.push(json!("perPage"));
        body_array.push(json!(params.per_page.unwrap_or(24)));
        body_array.push(json!("page"));
        body_array.push(json!(params.page.unwrap_or(1)));

        json!({
            "endpoint": "/classifieds/cars/search/searchByUrlParams",
            "options": {
                "method": "POST",
                "headers": {
                    "Content-Type": "application/json",
                    "User-Agent": "cars-node",
                    "X-App-Source": "frontline",
                    "X-DDM-EVENT-USER-AGENT": {},
                    "X-DDM-EVENT-ACCEPT-LANGUAGE": "en-US",
                    "X-MEMBER-ID": null,
                    "cookie": ""
                },
                "body": body_array
            }
        })
    }
}
```

**Response parsing:** Deserialize into a private wire type:

```rust
#[derive(Deserialize)]
struct CarsApiResponse {
    data: CarsApiData,
}

#[derive(Deserialize)]
struct CarsApiData {
    items: Vec<RawCarItem>,
}

#[derive(Deserialize)]
struct RawCarItem {
    id: Option<Value>,  // can be string or number
    title: Option<String>,
    price: Option<f64>,
    make: Option<String>,
    model: Option<String>,
    year: Option<u32>,
    mileage: Option<u32>,
    city: Option<String>,
    state: Option<String>,
    zip: Option<String>,
    photo: Option<String>,
    description: Option<String>,
    #[serde(rename = "sellerType")]
    seller_type: Option<String>,
}
```

Map `RawCarItem` → `CarListing` via `From` impl. Construct URL as `https://cars.ksl.com/listing/{id}`.

**Headers for outer request:**
```rust
headers.insert(header::CONTENT_TYPE, "application/json");
headers.insert(header::ORIGIN, "https://cars.ksl.com");
```

**Pagination:** Cars API is 1-indexed. `has_more` = items.len() >= per_page.

---

### 3. Listing Detail Parser — `src/parser.rs` (extend existing file)

Add function:

```rust
pub fn parse_listing_detail(html: &str, id: &str) -> Result<ListingDetail>
```

**HTML parsing strategy:**

The listing detail page at `https://classifieds.ksl.com/listing/{id}` embeds data in `window.detailPage.listingData`. Strategy:

1. **Primary approach:** Extract JSON from inline `<script>` tag containing `window.detailPage.listingData = {...}`:
   - Find `<script>` elements, search text for `listingData`
   - Extract the JSON object after the `=` sign (up to the next `;` or `</script>`)
   - Parse as JSON to get title, description, price, photos, seller info, condition, posted date

2. **Fallback approach (if JS data not found):** CSS selector scraping:
   - Title: `h1` or `[data-testid="listing-title"]`
   - Description: `[data-testid="listing-description"]` or `.description`
   - Price: element with price pattern (`$X,XXX`)
   - Photos: `img[src*="ksldigital.com"]` — collect all `src` attributes
   - Seller type: look for "Private" / "Dealer" text near seller section
   - Posted date: look for date pattern near "Listed" or "Posted" text
   - Condition: look for condition labels (New, Used - Excellent, etc.)

3. **Photo URL normalization:** Ensure full URLs (prepend `https:` if protocol-relative).

---

### 4. Client Trait Extension — `src/client/mod.rs`

```rust
pub mod classifieds;
pub mod cars;
pub mod rate_limiter;

use crate::error::Result;
use crate::types::{ClassifiedsSearchParams, SearchResults, CarsSearchParams, CarsSearchResults, ListingDetail};

pub trait KslClient: Send + Sync {
    async fn search_classifieds(&self, params: &ClassifiedsSearchParams) -> Result<SearchResults>;
    async fn search_cars(&self, params: &CarsSearchParams) -> Result<CarsSearchResults>;
    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail>;
}
```

Add `get_listing_detail` to `ClassifiedsClient`:
- `GET https://classifieds.ksl.com/listing/{id}`
- Pass through rate limiter
- Parse HTML with `parser::parse_listing_detail`

---

### 5. Tools — `src/tools/search.rs` (extend)

Add `search_cars` and `get_listing` tool methods to `KslMcpServer`.

**Modify `KslMcpServer` struct:**
```rust
#[derive(Clone)]
pub struct KslMcpServer {
    classifieds_client: ClassifiedsClient,
    cars_client: CarsClient,
}
```

**Add `SearchCarsInput`:**
```rust
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
```

**Add `GetListingInput`:**
```rust
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetListingInput {
    /// The listing ID (numeric string from the listing URL)
    pub id: String,
}
```

**Tool implementations:**
```rust
#[tool(description = "Search KSL Cars listings by make, model, year, price, mileage, and location.")]
async fn search_cars(&self, #[tool(aggr)] input: SearchCarsInput) -> String {
    let params = CarsSearchParams { /* map from input */ ..Default::default() };
    match self.cars_client.search_cars(&params).await {
        Ok(results) => serde_json::to_string(&results).unwrap_or_else(|e| e.to_string()),
        Err(e) => format!("Error: {}", e),
    }
}

#[tool(description = "Get full details for a KSL Classifieds listing including description, photos, and seller info.")]
async fn get_listing(&self, #[tool(aggr)] input: GetListingInput) -> String {
    match self.classifieds_client.get_listing_detail(&input.id).await {
        Ok(detail) => serde_json::to_string(&detail).unwrap_or_else(|e| e.to_string()),
        Err(e) => format!("Error: {}", e),
    }
}
```

---

### 6. Main — `src/main.rs` (extend)

```rust
use crate::client::cars::CarsClient;

let classifieds_client = ClassifiedsClient::new(&config);
let cars_client = CarsClient::new(&config);
let server = KslMcpServer::new(classifieds_client, cars_client);
```

Update `ServerInfo.instructions` to mention cars.

---

## File Summary

| File | Action |
|------|--------|
| `src/types.rs` | Add `Platform::Cars`, `ListingDetail`, `CarsSearchParams`, `CarListing`, `CarsSearchResults` |
| `src/client/mod.rs` | Add `pub mod cars;`, extend `KslClient` trait |
| `src/client/cars.rs` | **New** — `CarsClient` with `search_cars` |
| `src/parser.rs` | Add `parse_listing_detail` function |
| `src/tools/search.rs` | Add `CarsClient` to server, add `search_cars` + `get_listing` tools |
| `src/main.rs` | Construct `CarsClient`, pass to `KslMcpServer` |
| `tests/fixtures/listing_detail.html` | **New** — captured fixture for parser tests |
| `tests/fixtures/cars_search.json` | **New** — captured fixture for cars response parsing |

---

## Testing

1. **`parse_listing_detail`** — unit test with `tests/fixtures/listing_detail.html` fixture
2. **Cars response parsing** — unit test with `tests/fixtures/cars_search.json` fixture
3. **`build_request_body`** — unit test verifying flat key/value array structure
4. **Integration** — manual test: `cargo run`, call `search_cars` and `get_listing` via MCP client

---

## Key Constraints

- Cars API page is **1-indexed** (not 0-indexed like classifieds)
- Multiple makes/models are **semicolon-separated** (e.g. `"Ford;Toyota"`)
- Body array values can be **strings or numbers** — use `serde_json::Value`
- The `options.headers` in the body are **inner headers** (sent by the proxy to the backend), not the outer HTTP request headers
- Rate limiter must be shared or separate instance — use separate `RateLimiter` for cars endpoint (per-endpoint isolation per architecture)
- Listing detail fetch goes through the **same** classifieds rate limiter (same endpoint domain)
- All listing-sourced strings must be treated as untrusted (relevant for Stage 4 reports)
