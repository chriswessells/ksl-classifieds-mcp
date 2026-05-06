# Stage 1 Instructions — Core Search

## Pre-Implementation Prerequisites

Before any code is written:
1. **Capture HTML fixture**: Fetch a real KSL search results page and save to `tests/fixtures/search_results.html`. This is the ground truth for parser development.
2. **Capture known-good URL**: Record the exact URL used to fetch the fixture (with params). This anchors the URL builder tests.
3. **Remove `rusqlite` from Cargo.toml** — it compiles 150k lines of C and is not used until Stage 3.
4. **Remove `async-trait` from planned deps** — Rust 1.75+ supports native async fn in traits.

---

## Phase 1A: Foundation

### Work Item 1A.1 — Error Types

**Files:** `src/error.rs`

**Implementation:**
- `thiserror = "2"` in Cargo.toml
- `KslError` enum: `Http(reqwest::Error)`, `RateLimited { reason: String }`, `DailyCapExceeded { cap: u32 }`, `Parse { context: String }`, `Config(String)`, `Io(std::io::Error)`
- `pub type Result<T> = std::result::Result<T, KslError>`

**Acceptance Criteria:**
- Compiles, `From` impls work via `?`

**Dependencies:** None

---

### Work Item 1A.2 — Domain Types

**Files:** `src/types.rs`

**Implementation:**
- `Platform` enum: `Classifieds` only (single variant, `#[non_exhaustive]`)
- `SortOrder` enum: `Newest`, `Oldest`, `PriceLow`, `PriceHigh` with `to_ksl_param() -> &'static str` returning `"0"`, `"1"`, `"2"`, `"3"`
- `Listing` struct: id, title, price (Option<f64>), city (Option), state (Option), url, image_url (Option), category (Option), favorites_count (Option<u32>), platform
- `ClassifiedsSearchParams`: keyword (Option<String>), category (Option<u32>), sub_category (Option<u32>), price_from (Option<u32>), price_to (Option<u32>), zip (Option<String>), miles (Option<u32>), sort (Option<SortOrder>), page (Option<u32>), per_page (Option<u32>), seller_type (Option<String>), has_photos (Option<bool>)
- `SearchResults`: listings (Vec<Listing>), page (u32), has_more (bool)
- `Category`: id (u32), name (String)
- All types: `#[derive(Debug, Clone, Serialize, Deserialize)]`

**Acceptance Criteria:**
- All types serialize/deserialize with serde_json
- `#[non_exhaustive]` on Platform

**Dependencies:** None

---

### Work Item 1A.3 — Configuration

**Files:** `src/config.rs`

**Implementation:**
- `toml = "0.8"` in Cargo.toml
- Private `RawConfig` (all fields `Option<T>`) for TOML deserialization
- Public `Config` struct with resolved values
- `Config::load()`:
  1. Resolve `~/.config/ksl-mcp/config.toml`
  2. Missing file → log info, return defaults
  3. Parse error → log warn WITH the error message, return defaults
  4. Success → merge with defaults (None fields get default)
- `Config::defaults()` returns hardcoded values
- **Security constraint:** After resolving `data_dir`, validate it is within `$HOME`. If not, log warn and substitute default.
- Log effective config values at INFO on load (timeouts, cap, data_dir)

**Defaults:**
- user_agent: Safari macOS UA string
- connect_timeout_secs: 10
- request_timeout_secs: 30
- min_delay_ms: 3000
- max_delay_ms: 8000
- daily_request_cap: 500
- data_dir: `~/.local/share/ksl-mcp/`

**Acceptance Criteria:**
- Missing config → defaults, no panic
- Malformed TOML → defaults, logs warning with parse error
- `data_dir="/etc"` → rejected, uses default, logs warning
- Partial config → merges with defaults

**Dependencies:** None

---

### Work Item 1A.4 — Categories

**Files:** `src/categories.rs`

**Implementation:**
- `pub fn all_categories() -> &'static [Category]` (use `once_cell::Lazy` or `std::sync::LazyLock`)
- All 29 categories from API_RESEARCH.md with correct IDs

**Acceptance Criteria:**
- Returns 29 categories
- Known IDs correct (Announcements=1, Electronics=345, FREE=349, Cycling=736)

**Dependencies:** 1A.2

---

### Work Item 1A.5 — Module Structure & KslClient Trait

**Files:** `src/main.rs`, `src/client/mod.rs`

**Implementation:**
- Module declarations in main.rs
- `KslClient` trait with native async fn (no async-trait crate):
  ```rust
  pub trait KslClient: Send + Sync {
      async fn search_classifieds(&self, params: &ClassifiedsSearchParams) -> Result<SearchResults>;
  }
  ```
  Note: If `dyn KslClient` is needed for test mocks and native async traits don't support it yet in stable, use a concrete generic `<C: KslClient>` on the server struct instead.
- `cargo check` passes

**Acceptance Criteria:**
- Module tree compiles
- Trait is usable with concrete generic (or dyn if stable supports it)

**Dependencies:** 1A.1, 1A.2

---

## Phase 1B: HTTP Infrastructure

### Work Item 1B.1 — Rate Limiter

**Files:** `src/client/rate_limiter.rs`

**Implementation:**
- `RateLimiter::new(config)` — loads persisted state from file
- `acquire()`:
  1. Lock mutex briefly to read state (check backoff, check cap, compute delay)
  2. **Release mutex before sleeping** (critical: no await while holding lock)
  3. Sleep for computed delay
  4. Re-acquire mutex to update `last_request_at`, increment counter
  5. Persist state synchronously (best-effort, log on failure)
- `record_success()` — reset consecutive_failures
- `record_failure()` — only called for 429 (NOT for 503/transient). Increment failures, compute backoff with ±20% jitter
- State persistence file: `{data_dir}/rate_state.txt` with format:
  ```
  YYYY-MM-DD
  {count}
  {backoff_until_unix_secs_or_0}
  ```
- `daily_count_date` typed as `chrono::NaiveDate` internally
- Date rollover resets counter
- Backoff state persisted and honored across restarts

**Critical constraints:**
- Mutex NEVER held across any `await` point (sleep or I/O)
- `record_failure()` only triggered by 429, not by transient errors (503, connection reset)
- Persist is synchronous (blocking file write) — acceptable at 3-8s request intervals

**Acceptance Criteria:**
- Sequential acquires spaced by min_delay_ms
- Daily cap enforcement
- Backoff doubles on consecutive 429s (with jitter)
- Success resets backoff
- Date rollover resets count
- Process restart during backoff → backoff still honored
- Two concurrent acquire() calls don't block each other for sleep duration

**Dependencies:** 1A.1, 1A.3

---

### Work Item 1B.2 — Classifieds HTTP Client

**Files:** `src/client/classifieds.rs`

**Implementation:**
- `ClassifiedsClient::new(config)`:
  - Build `reqwest::Client` with BOTH:
    - `.connect_timeout(Duration::from_secs(config.connect_timeout_secs))`
    - `.timeout(Duration::from_secs(config.request_timeout_secs))`
  - Set default headers from API_RESEARCH.md (User-Agent, Accept, Accept-Language, Referer, DNT, Sec-Fetch-*)
  - **TLS validation must remain enabled — do NOT call `danger_accept_invalid_certs`**
- `build_search_url(params)`:
  - Base: `https://classifieds.ksl.com/v2/search`
  - Append path segments for non-None params: keyword, category, subCategory, priceFrom, priceTo, zip, miles, sort, page, perPage
  - **All string values MUST be percent-encoded** using `percent_encoding::utf8_percent_encode` with `NON_ALPHANUMERIC` or `PATH_SEGMENT` set
  - Add `percent-encoding = "2"` to Cargo.toml
- `search_classifieds(params)`:
  1. `rate_limiter.acquire()?`
  2. Build URL
  3. Send GET
  4. **Response size check:** reject if body > 2MB
  5. Status handling with retry loop (max 2 retries for transient):
     - 429 → `record_failure()`, return `RateLimited` (no retry)
     - 503/502/504 or connection error → retry after 2s (up to 2 retries), then return error
     - Other non-200 → return Http error
  6. On 200: `record_success()`, parse body
  7. Return SearchResults

**Acceptance Criteria:**
- URL built correctly (test against known-good captured URL)
- String params percent-encoded (test: keyword="foo/bar?x=1" → encoded)
- Rate limiter called before every request
- 429 triggers backoff, 503 does NOT (retries instead)
- Both timeouts configured (test: stalled response returns error within timeout)
- Response > 2MB rejected
- TLS validation enabled (no danger_ methods)

**Dependencies:** 1A.1, 1A.2, 1A.3, 1A.5, 1B.1, 1C.1

---

## Phase 1C: HTML Parsing

### Work Item 1C.0 — Capture Fixture (PREREQUISITE)

**Files:** `tests/fixtures/search_results.html`, `tests/fixtures/CAPTURE_NOTES.md`

**Task:** Manually fetch a real KSL search results page and save it.

**Implementation:**
- Run a search on classifieds.ksl.com in a browser
- Save the full HTML response
- Document in CAPTURE_NOTES.md: URL used, date captured, number of results expected
- This is the ground truth for all parser tests

**Acceptance Criteria:**
- File exists and contains real KSL HTML
- CAPTURE_NOTES.md documents the source URL and expected result count

**Dependencies:** None (can run in parallel with 1A)

---

### Work Item 1C.1 — Search Results Parser

**Files:** `src/parser.rs`

**Implementation:**
- `scraper = "0.20"` in Cargo.toml
- `parse_search_results(html: &str) -> Result<Vec<Listing>>`:
  1. Parse with `scraper::Html::parse_document`
  2. Select `a[role="listitem"]` (validate against fixture — adjust selector if needed)
  3. Extract per listing: id (from href), title (aria-label), price, city/state, url, image_url, favorites_count
  4. Skip malformed listings (log at debug), continue with rest
  5. **Silent failure detection:** If `listings.is_empty()` AND `html.len() > 5000`, log at WARN: "parsed 0 listings from {len}B response — possible selector mismatch"
  6. Set `platform: Platform::Classifieds` on all
- `has_more_results(listings, per_page)`: `listings.len() >= per_page as usize`

**Acceptance Criteria:**
- Parses fixture HTML correctly (count matches CAPTURE_NOTES.md)
- All fields of first listing match expected values
- Missing optional fields handled gracefully
- Malformed listings skipped without failing entire parse
- Large response with 0 results → WARN log (not silent empty vec)

**Dependencies:** 1A.1, 1A.2, 1C.0

---

## Phase 1D: MCP Integration

### Work Item 1D.1 — MCP Server & Tools

**Files:** `src/tools/mod.rs`, `src/tools/search.rs`, `src/main.rs`, `README.md`

**Implementation:**
- `KslMcpServer<C: KslClient>` struct holding the client
- Register tools via rmcp macros/API: `search_classifieds`, `list_categories`
- `search_classifieds` tool:
  - Input: keyword (string), category (number), price_from (number), price_to (number), zip (string), miles (number), sort (string enum), page (number) — all optional
  - Map to ClassifiedsSearchParams, call client, return JSON
  - Errors → MCP tool error with human-readable message
- `list_categories` tool:
  - No input
  - Return JSON array of {id, name}
- `main.rs`:
  - Init tracing subscriber (respect RUST_LOG env var)
  - Config::load()
  - Construct ClassifiedsClient
  - Construct KslMcpServer
  - Start rmcp stdio transport
  - Log startup: config path, data dir, effective timeouts, daily cap
- **MCP client config snippet** in README.md:
  ```json
  {
    "mcpServers": {
      "ksl-classifieds": {
        "command": "ksl-classifieds-mcp",
        "args": []
      }
    }
  }
  ```

**Acceptance Criteria:**
- Binary starts and completes MCP handshake
- `list_categories` returns 29 categories
- `search_classifieds` with keyword returns structured results from live KSL
- Rate limit errors → user-friendly tool error
- Missing config → starts with defaults (logged)
- README contains MCP client config snippet
- `cargo install --git <repo> --locked` produces working binary named `ksl-classifieds-mcp`

**Dependencies:** All previous work items

---

## Cargo.toml (Stage 1 Final)

```toml
[package]
name = "ksl-classifieds-mcp"
version = "0.1.0"
edition = "2021"
description = "MCP server for searching KSL Classifieds and tracking listing prices"
license = "MIT"

[dependencies]
rmcp = { version = "0.1", features = ["server", "transport-io"] }
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
scraper = "0.20"
toml = "0.8"
chrono = { version = "0.4", features = ["serde"] }
rand = "0.8"
percent-encoding = "2"
```

Note: `rusqlite`, `axum`, `askama` are NOT included — added in their respective stages.

---

## What NOT to Implement in Stage 1

- No SQLite / rusqlite
- No tracking tools
- No cars client
- No get_listing detail
- No report server (axum)
- No saved searches
- No Platform::Cars variant
