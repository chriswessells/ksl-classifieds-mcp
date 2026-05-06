# Stage 1 Design — Core Search

## Overview

Stage 1 delivers a working MCP server that searches KSL Classifieds via HTML scraping and returns structured results. No database persistence is required for search functionality — the daily request counter uses a simple file-based approach, and if that fails, the server still works (just without cap enforcement).

## Phase Breakdown

| Phase | Name | Delivers |
|-------|------|----------|
| 1A | Foundation | Domain types, config loading, error types, project structure |
| 1B | HTTP Infrastructure | KSL HTTP client trait, rate limiter, reqwest implementation |
| 1C | HTML Parsing | Scraper-based parser for search results HTML |
| 1D | MCP Integration | rmcp server, tool registration, search_classifieds + list_categories tools |

Phases 1A is prerequisite for all others. 1B and 1C can run in parallel after 1A. 1D depends on 1B + 1C.

---

## Component Interfaces

### 1. Configuration (`src/config.rs`)

```rust
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub user_agent: String,
    pub connect_timeout_secs: u64,
    pub request_timeout_secs: u64,
    pub min_delay_ms: u64,
    pub max_delay_ms: u64,
    pub daily_request_cap: u32,
    pub data_dir: PathBuf,
}

impl Config {
    /// Load from ~/.config/ksl-mcp/config.toml, falling back to defaults.
    /// Never fails — logs warnings on parse errors and uses defaults.
    pub fn load() -> Self;

    /// Returns default configuration values.
    pub fn defaults() -> Self;
}
```

**Defaults:**
- `user_agent`: `"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2.1 Safari/605.1.15"`
- `connect_timeout_secs`: 10
- `request_timeout_secs`: 30
- `min_delay_ms`: 3000
- `max_delay_ms`: 8000
- `daily_request_cap`: 500
- `data_dir`: `~/.local/share/ksl-mcp/`

### 2. Error Types (`src/error.rs`)

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KslError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Rate limited: {reason}")]
    RateLimited { reason: String },

    #[error("Daily request cap exceeded ({cap} requests)")]
    DailyCapExceeded { cap: u32 },

    #[error("Parse error: {context}")]
    Parse { context: String },

    #[error("Config error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, KslError>;
```

### 3. Domain Types (`src/types.rs`)

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    Classifieds,
    Cars,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SortOrder {
    Newest,
    Oldest,
    PriceLow,
    PriceHigh,
}

impl SortOrder {
    /// Map to KSL's numeric sort parameter
    pub fn to_ksl_param(&self) -> u8;
}

/// A classified listing (stable internal type)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    pub id: String,
    pub title: String,
    pub price: Option<f64>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub url: String,
    pub image_url: Option<String>,
    pub category: Option<String>,
    pub favorites_count: Option<u32>,
    pub platform: Platform,
}

/// Search parameters for classifieds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifiedsSearchParams {
    pub keyword: Option<String>,
    pub category: Option<u32>,
    pub sub_category: Option<u32>,
    pub price_from: Option<u32>,
    pub price_to: Option<u32>,
    pub zip: Option<String>,
    pub miles: Option<u32>,
    pub sort: Option<SortOrder>,
    pub page: Option<u32>,  // 0-indexed
    pub per_page: Option<u32>,
    pub seller_type: Option<String>,
    pub has_photos: Option<bool>,
}

/// Search results with pagination metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub listings: Vec<Listing>,
    pub page: u32,
    pub has_more: bool,
}

/// A category definition (hardcoded)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: u32,
    pub name: String,
}
```

### 4. HTTP Client Trait (`src/client/mod.rs`)

```rust
use crate::types::{ClassifiedsSearchParams, SearchResults};
use crate::error::Result;
use async_trait::async_trait;

#[async_trait]
pub trait KslClient: Send + Sync {
    /// Search classifieds, returning parsed results.
    /// Handles rate limiting internally.
    async fn search_classifieds(&self, params: &ClassifiedsSearchParams) -> Result<SearchResults>;
}
```

### 5. Rate Limiter (`src/client/rate_limiter.rs`)

```rust
use crate::error::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Per-endpoint rate limiting state
pub struct RateLimiter {
    state: Arc<Mutex<RateLimiterState>>,
    config: RateLimiterConfig,
}

struct RateLimiterConfig {
    min_delay_ms: u64,
    max_delay_ms: u64,
    daily_cap: u32,
    backoff_initial_ms: u64,  // 30_000
    backoff_max_ms: u64,      // 300_000
    data_dir: std::path::PathBuf,
}

struct RateLimiterState {
    last_request_at: Option<tokio::time::Instant>,
    daily_count: u32,
    daily_count_date: String,  // YYYY-MM-DD
    backoff_until: Option<tokio::time::Instant>,
    consecutive_failures: u32,
}

impl RateLimiter {
    pub fn new(config: &crate::config::Config) -> Self;

    /// Wait until it's safe to make a request. Returns Err if daily cap exceeded.
    pub async fn acquire(&self) -> Result<()>;

    /// Record a successful request.
    pub async fn record_success(&self);

    /// Record a failure (429/503) — triggers exponential backoff.
    pub async fn record_failure(&self);

    /// Load daily count from file (graceful: returns 0 on any error).
    fn load_daily_count(data_dir: &std::path::Path) -> (u32, String);

    /// Persist daily count to file (best-effort, failures logged).
    async fn persist_daily_count(&self);
}
```

### 6. Classifieds HTTP Client (`src/client/classifieds.rs`)

```rust
use crate::client::KslClient;
use crate::client::rate_limiter::RateLimiter;
use crate::parser;
use crate::types::{ClassifiedsSearchParams, SearchResults};
use crate::error::Result;
use crate::config::Config;

pub struct ClassifiedsClient {
    http: reqwest::Client,
    rate_limiter: RateLimiter,
}

impl ClassifiedsClient {
    pub fn new(config: &Config) -> Self;

    /// Build the URL path from search params.
    /// Pattern: /v2/search/keyword/{kw}/priceFrom/{n}/priceTo/{n}/zip/{z}/miles/{m}/page/{p}
    /// Omits segments for None params.
    fn build_search_url(&self, params: &ClassifiedsSearchParams) -> String;
}

#[async_trait]
impl KslClient for ClassifiedsClient {
    async fn search_classifieds(&self, params: &ClassifiedsSearchParams) -> Result<SearchResults>;
}
```

### 7. HTML Parser (`src/parser.rs`)

```rust
use crate::types::Listing;
use crate::error::Result;

/// Parse KSL classifieds search results HTML into listings.
/// Selects all `<a role="listitem">` elements and extracts fields.
/// Returns empty vec on no results (not an error).
/// Returns Err only on fundamentally unparseable HTML (e.g., empty body).
pub fn parse_search_results(html: &str) -> Result<Vec<Listing>>;

/// Determine if there are more pages of results.
/// Heuristic: if we got a full page of results (24 items), assume more exist.
pub fn has_more_results(listings: &[Listing], per_page: u32) -> bool;
```

### 8. Categories (`src/categories.rs`)

```rust
use crate::types::Category;

/// Returns the complete hardcoded list of KSL Classifieds categories.
/// No network call — this data is static.
pub fn all_categories() -> Vec<Category>;
```

### 9. MCP Server (`src/tools/mod.rs`, `src/tools/search.rs`)

```rust
// src/tools/mod.rs
use rmcp::ServerHandler;
use std::sync::Arc;
use crate::client::KslClient;

pub struct KslMcpServer {
    client: Arc<dyn KslClient>,
}

impl KslMcpServer {
    pub fn new(client: Arc<dyn KslClient>) -> Self;
}

// The rmcp #[tool] macro handles registration.
// Tools: search_classifieds, list_categories
```

```rust
// src/tools/search.rs — tool implementations

/// search_classifieds tool
/// Input: keyword, category, price_from, price_to, zip, miles, sort, page
/// Output: JSON array of Listing objects + pagination info
/// Errors: rate limited, daily cap, HTTP failure, parse failure

/// list_categories tool
/// Input: none
/// Output: JSON array of Category objects
/// Errors: none (hardcoded data)
```

---

## Error Handling Strategy

| Layer | Strategy |
|-------|----------|
| Config loading | Never fails. Log warning, use defaults. |
| Rate limiter file I/O | Best-effort persistence. Failure → log, continue with in-memory count. |
| HTTP requests | Return `KslError::Http`. Caller decides retry. |
| 429/503 responses | Trigger per-endpoint backoff via `record_failure()`. |
| HTML parsing | Return partial results if some listings parse, skip malformed ones. Log skipped. |
| Daily cap | Return `KslError::DailyCapExceeded` — tool returns user-friendly message. |
| MCP tool errors | Map `KslError` to MCP tool error response with human-readable message. |

**Graceful Degradation:** Stage 1 has no hard DB dependency. The daily counter file is best-effort. If it can't be read/written, the server operates without cap enforcement (logged at WARN).

---

## Component Connection Diagram

```
main.rs
  ├── Config::load()
  ├── ClassifiedsClient::new(&config)
  │     └── RateLimiter::new(&config)
  ├── KslMcpServer::new(Arc::new(client))
  └── rmcp stdio transport start
        └── Tool calls → KslMcpServer
              ├── search_classifieds → client.search_classifieds(params)
              │     ├── rate_limiter.acquire()
              │     ├── reqwest GET → HTML
              │     ├── parser::parse_search_results(html)
              │     └── rate_limiter.record_success/failure()
              └── list_categories → categories::all_categories()
```

---

## Dependencies (Cargo.toml additions for Stage 1)

```toml
[dependencies]
scraper = "0.20"
toml = "0.8"
thiserror = "2"
async-trait = "0.1"
chrono = { version = "0.4", features = ["serde"] }
```

Note: `rusqlite` is already in Cargo.toml but NOT used in Stage 1. It stays for future stages. The daily counter uses a plain text file in `data_dir`.
