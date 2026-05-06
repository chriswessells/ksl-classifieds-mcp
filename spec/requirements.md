# Requirements Specification — ksl-classifieds-mcp

## Product Overview

A local MCP server (Rust, stdio transport) that searches KSL Classifieds and KSL Cars, tracks item prices over time, detects sold/removed listings, and presents results via interactive HTML reports. Designed for use with Kiro/Claude as the MCP client.

## Supported Platforms

- macOS (arm64, x86_64) — primary
- Linux (x86_64) — secondary
- Windows — not supported

## Constraints

- Single binary, no runtime dependencies
- Local-only (no cloud infrastructure)
- No authentication required for search (KSL has no public API — uses reverse-engineered endpoints)
- Must respect rate limits to avoid detection (3-8s random delay, max 500 req/day)
- Action hash (`Next-Action` header) is deployment-dependent and must be auto-recoverable (max 2 attempts per invocation)
- All multi-table writes MUST execute within a single SQLite transaction
- All KSL-sourced data MUST be HTML-escaped before template insertion
- All outbound URL fetches MUST validate scheme (https only) and resolved IP (block private/loopback/link-local)
- All database queries MUST use parameterized statements (no string interpolation in SQL)
- HTTP timeouts: connect 10s, total request 30s (configurable)

## Technology Stack

| Component | Choice |
|-----------|--------|
| Language | Rust |
| MCP SDK | `rmcp` (official, stdio transport, pinned version) |
| HTTP Client | `reqwest` (async, cookies, timeouts) |
| HTML Parsing | `scraper` (CSS selectors) |
| HTML Templating | `askama` (auto-escaping) |
| Database | SQLite (`rusqlite`, bundled, WAL mode) |
| Local UI | `axum` (singleton report server) |
| Distribution | GitHub Releases (pre-built binaries) + `cargo install --locked` |

## Type Safety Requirements

The following domain values MUST be Rust enums with `serde` + `rusqlite` ToSql/FromSql impls:
- `Platform` — `Classifieds | Cars`
- `TrackingStatus` — `Active | Sold | Removed`
- `SortOrder` — `Newest | Oldest | PriceLow | PriceHigh`
- `Condition` — `New | UsedExcellent | UsedGood | UsedFair | UsedPoor | UsedDamaged | NA`

## Data Isolation

Each client module (`client/classifieds.rs`, `client/cars.rs`) owns private wire-format types. A `From<RawX> for Listing` impl maps wire types to the stable internal `Listing` type. No wire-format field appears outside the `client/` module.

## Rate Limiting

| Behavior | Implementation |
|----------|---------------|
| Request spacing | Random delay 3-8s between requests |
| Burst protection | Max 1 concurrent request per endpoint |
| Backoff | Per-endpoint exponential backoff on 429/503 (start 30s, max 5min) |
| Daily cap | 500 req/day (persistent across restarts via SQLite) |
| Recovery requests | Tracked separately, do NOT decrement daily search quota |
| Timeouts | Connect 10s, total 30s (configurable) |

## Timeout Policy

| Context | Connect | Total | Configurable |
|---------|---------|-------|-------------|
| KSL search/detail | 10s | 30s | Yes |
| External URL fetch (research_item) | 10s | 15s | Yes |
| Report server startup | — | 500ms | No |

## Observability

- All status transitions logged at INFO: listing_id, old_status, new_status, trigger, timestamp
- Startup logs: DB path, config path, schema version, platform
- Action hash changes logged at WARN
- Rate limit exhaustion logged at ERROR

## Test Strategy

- Parsing functions are pure functions testable without HTTP
- `tests/fixtures/` directory with captured KSL HTML/JSON responses
- HTTP client behind a trait for mock injection in tests
- Tests for CRITICAL and HIGH cases only (transaction atomicity, hash recovery bounds, XSS escaping, SSRF blocking)

## Distribution

- Primary: GitHub Releases with pre-built binaries (macOS arm64, Linux x86_64)
- Secondary: `cargo install --locked --git <repo>`
- CI: GitHub Actions builds on tag push
- `Cargo.lock` committed to repository
- `rust-toolchain.toml` specifies MSRV

---

## Stage 1: Core Search

**Goal:** Working MCP server that can search KSL Classifieds and return structured results.

**Deliverables:**
1. MCP server binary with stdio transport using `rmcp`
2. `search_classifieds` tool — keyword, category, price range, location, sort, pagination
3. `list_categories` tool — returns available categories/subcategories (hardcoded, no network call)
4. KSL HTTP client with:
   - GET URL pattern search (`/v2/search/keyword/{keyword}/...`) + HTML parsing via `scraper`
   - Rate limiting (random 3-8s delay, max 1 concurrent per endpoint, per-endpoint backoff)
   - Browser-like headers (configurable User-Agent)
   - HTTP timeouts (connect 10s, total 30s)
   - Daily request cap (persistent in SQLite metadata table)
5. Configuration file (`~/.config/ksl-mcp/config.toml`) with fallback to defaults on missing/malformed
6. Graceful degradation: if DB init fails, search tools still work (tracking tools return error)
7. Wire-format isolation: private `RawClassifiedsItem` type with `From<Raw> for Listing` mapping

**Acceptance Criteria:**
- `search_classifieds` returns listings with: id, title, price, location, image_url, url, category, favorites_count
- Pagination works via page number
- Rate limiter enforces per-endpoint delays and backoff
- Binary starts and registers tools via stdio MCP handshake
- Missing config file → server starts with logged defaults
- DB failure → server starts, search works, tracking tools return structured error

---

## Stage 2: Listing Details & Cars

**Goal:** Full listing detail retrieval and KSL Cars support.

**Deliverables:**
1. `get_listing` tool — fetch full detail page, extract description, photos, seller info
2. `search_cars` tool — search KSL Cars via their JSON proxy API
3. HTML parsing for listing detail pages (CSS selectors)
4. Image URL extraction and formatting

**Acceptance Criteria:**
- `get_listing` returns: title, description, price, photos[], location, seller_type, posted_date, condition
- `search_cars` returns: title, price, make, model, year, mileage, location, photo_url
- Cars API uses the `POST cars.ksl.com/nextjs-api/proxy` endpoint with flat key/value body

---

## Stage 3: Price Tracking

**Goal:** Persistent tracking of listings with price history.

**Deliverables:**
1. SQLite database initialization (WAL mode, schema migrations via `schema_version` table)
2. `track_item` tool — save listing to watch list, record initial price (idempotent: duplicate calls produce exactly one tracked_items row and one initial snapshot)
3. `untrack_item` tool — remove from watch list
4. `list_tracked_items` tool — list all tracked items with current state (single JOIN query, not N+1)
5. `get_price_history` tool — price snapshots over time for a listing
6. `mark_as_sold` tool — manual sold marking
7. Sold/removed auto-detection:
   - "SOLD" indicator on listing page → mark sold (only after confirmed detection)
   - Confirmed 404 (2 independent fetches) → mark removed
   - 5xx/connection errors do NOT trigger status transitions
8. All status transitions logged at INFO with: listing_id, old_status, new_status, trigger, timestamp
9. `status_changed_at` column on tracked_items

**Transaction Boundaries:**
- `track_item`: single transaction wrapping listings INSERT + tracked_items INSERT + price_snapshots INSERT
- Price check update: single transaction wrapping price_snapshots INSERT + tracked_items UPDATE
- `mark_as_sold`: single transaction wrapping tracked_items UPDATE + final price_snapshots INSERT
- Sold/removed auto-detection: single transaction wrapping status change + final snapshot

**Idempotency:**
- `track_item` checks for existing tracked_items row; if exists, returns current state without inserting duplicate snapshot
- Price snapshot deduplication: skip INSERT if same listing_id + same price exists within last 60 seconds

**Data Model:**
```sql
CREATE TABLE schema_version (
  version INTEGER NOT NULL
);

CREATE TABLE listings (
  id TEXT PRIMARY KEY,
  platform TEXT NOT NULL,
  title TEXT NOT NULL,
  description TEXT,
  price REAL,
  photos TEXT,
  city TEXT, state TEXT, zip TEXT,
  category TEXT, sub_category TEXT,
  seller_type TEXT, posted_date TEXT,
  url TEXT NOT NULL,
  favorites_count INTEGER,
  make TEXT, model TEXT, year INTEGER, mileage INTEGER,
  first_seen_at TEXT NOT NULL,
  last_fetched_at TEXT NOT NULL
);

CREATE TABLE tracked_items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  listing_id TEXT NOT NULL REFERENCES listings(id),
  platform TEXT NOT NULL,
  notes TEXT,
  first_seen_price REAL,
  current_price REAL,
  first_seen_at TEXT NOT NULL,
  last_checked_at TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  status_changed_at TEXT,
  sold_price REAL,
  UNIQUE(listing_id)
);

CREATE TABLE price_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  listing_id TEXT NOT NULL REFERENCES listings(id),
  price REAL NOT NULL,
  recorded_at TEXT NOT NULL
);

CREATE INDEX idx_price_snapshots_listing_time ON price_snapshots(listing_id, recorded_at);
```

**Acceptance Criteria:**
- Tracked items persist across server restarts
- Price history accumulates on repeated checks
- Sold detection works for confirmed indicator and confirmed 404 (not transient errors)
- `list_tracked_items` shows price change from first seen (single query, not N+1)
- `track_item` is idempotent — calling twice produces no duplicate data
- Partial write failure leaves DB in clean state (transaction rollback)

---

## Stage 4: Interactive Reports

**Goal:** Visual HTML report for browsing search results with photo thumbnails and one-click tracking.

**Deliverables:**
1. `browse_search_results` tool — run search, generate HTML, open in browser
2. `get_pending_selections` tool — check if user submitted selections
3. Singleton `axum` HTTP server (127.0.0.1, OS-assigned port, started lazily on first call)
4. Self-contained HTML report via `askama` (auto-escaping, grid layout, checkboxes, submit button)
5. Security headers: `Content-Security-Policy: default-src 'self'; img-src https://image.ksldigital.com https://img.ksl.com; script-src 'none'; style-src 'unsafe-inline'`, `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`
6. CSRF token: 128-bit CSPRNG (`OsRng`), stored server-side, embedded in hidden form field (NOT in URL), single-use, validated on POST
7. Form POST handler persists selections to SQLite `pending_selections` table BEFORE returning 200
8. Platform-aware browser open: `open` (macOS), `xdg-open` (Linux), error on unsupported

**Report Server Lifecycle:**
- Singleton: one axum instance for the MCP server lifetime, started lazily
- Each report gets a unique path (`/report/{uuid}`)
- Report data auto-cleaned after 10 minutes
- Startup failure returns tool error within 500ms (no hang)
- Only one active report at a time; new `browse_search_results` invalidates previous report

**Acceptance Criteria:**
- Report opens in default browser with listing photos, prices, locations
- All listing-sourced text is HTML-escaped (test: `<script>alert(1)</script>` in title renders as escaped text)
- User can check items and submit → items are tracked
- CSRF token validated on POST; missing/invalid token → 403
- Server startup failure → immediate tool error (not a 10-minute hang)
- Works on macOS and Linux; returns actionable error on unsupported platforms

---

## Stage 5: Saved Searches & Stats

**Goal:** Saved search persistence and aggregate statistics.

**Deliverables:**
1. Saved searches table and CRUD (`save_search`, `list_saved_searches`, `delete_saved_search`, `run_saved_search`)
2. `get_sales_stats` tool — aggregate queries (avg days-to-sell, price drop frequency, category averages)

**Data Model (addition):**
```sql
CREATE TABLE saved_searches (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  parameters TEXT NOT NULL,
  platform TEXT NOT NULL,
  last_run_at TEXT,
  created_at TEXT NOT NULL
);
```

**Acceptance Criteria:**
- Saved searches persist and can be re-run via `run_saved_search`
- `get_sales_stats` returns meaningful aggregates from tracked data
- Saved search parameters are deserialized into typed structs, never interpolated raw into queries

---

## Out of Scope

- Cloud deployment (future: Bedrock AgentCore)
- Background polling/cron (future: Phase 2)
- Push notifications (future: email/Discord/SMS)
- Image analysis / ML models
- Authentication with KSL (not needed for search)
- Windows support
- `research_item` URL fetching (LLM already handles product research conversationally; can be added to roadmap if needed)
