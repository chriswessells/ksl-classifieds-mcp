# KSL Classifieds MCP — Specification

## Overview

A local MCP server written in Rust that provides tools for searching KSL Classifieds and KSL Cars, tracking item prices over time, and identifying sales patterns. Communicates via stdio transport using the official `rmcp` crate.

---

## Technology Stack

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Language | Rust | Performance, single binary distribution, strong typing |
| MCP SDK | `rmcp` (official, v1.6+) | Official SDK, macro-driven, stdio support, 4.7M+ downloads |
| Transport | stdio | Local use with Kiro/Claude, zero network overhead |
| HTTP Client | `reqwest` | Async, cookie support, custom headers |
| HTML Parsing | `scraper` | CSS selector-based, fast, well-maintained |
| Database | **SQLite** (`rusqlite`) | See [Database Decision](#database-decision) below |
| Local UI Server | `axum` | Lightweight, async, serves interactive HTML reports |
| Distribution | `cargo install` from git or crates.io | Single binary, no runtime deps |

### Database Decision

| Option | Pros | Cons | Verdict |
|--------|------|------|---------|
| **SQLite** (`rusqlite`) | Battle-tested, 25+ years, SQL queries, great tooling, tiny footprint (+1-2MB), fast compile, inspectable with any SQLite browser | Schema migrations needed | **Selected** — our data model is relational (tracked_item has many price_snapshots, saved_search has parameters). SQL handles price history, joins, and aggregates perfectly. |
| **SurrealDB embedded** | Multi-model, flexible schema, graph queries | +20-30MB binary, slower compile, younger project, breaking changes | Overkill — the "graph" relationships are just foreign keys. Pattern analysis lives in the LLM, not the DB. |
| **redb** | Pure Rust, fast KV, ACID | No query language, manual indexing | Too low-level |
| **sled** | Pure Rust, embedded | Unstable API, maintenance concerns | Not recommended |

---

## Rate Limiting Strategy

Goal: Appear as a normal human browser user, not a bot.

| Behavior | Implementation |
|----------|---------------|
| Request spacing | Random delay between 3-8 seconds between requests |
| Burst protection | Max 1 concurrent request to KSL at any time |
| Session simulation | Maintain cookies across requests within a session |
| Headers | Full browser-like header set (UA, Accept, Referer, Sec-Fetch-*) |
| Backoff | Exponential backoff on 429/503 responses (start 30s, max 5min) |
| Daily cap | Configurable max requests/day (default: 500) |

Implemented as a `RateLimiter` middleware wrapping the HTTP client.

---

## Interactive Report UI

Search results are presented via an interactive HTML report served from a temporary local HTTP server. This allows visual browsing of listings with photos and one-click item selection.

### Flow

1. User requests a search (via conversation)
2. MCP server fetches results from KSL
3. Server generates an HTML report with:
   - Grid/list of listings with thumbnail photos, prices, descriptions, locations
   - Checkbox next to each listing
   - "Track Selected" submit button
4. Server spawns a temporary `axum` listener on `127.0.0.1:{random_port}`
5. Server opens the report URL in the default browser (`open` on macOS)
6. User reviews listings visually, checks items of interest, clicks submit
7. Form POSTs selections back to the local server
8. Server processes selections (calls `track_item` internally for each)
9. Listener shuts down
10. MCP tool returns confirmation of which items were tracked

### Implementation Details

- **Server lifetime**: Spawns on tool call, shuts down after form submission or 10-minute timeout
- **Port**: Random available port (OS-assigned), returned in the tool response URL
- **HTML**: Self-contained single file (inline CSS/JS, no external deps)
- **Photos**: Loaded directly from KSL's image CDN (no proxying needed, browser fetches them)
- **Security**: Binds only to `127.0.0.1`, includes a one-time CSRF token in the form

### MCP Tools for Report UI

#### `browse_search_results`

Run a search and open an interactive HTML report in the browser for visual review and item selection.

**Parameters:** Same as `search_classifieds` or `search_cars` (keyword, category, price range, etc.) plus:
```json
{
  "platform": "string (optional) — 'classifieds' | 'cars', default: 'classifieds'"
}
```

**Returns:** URL of the report + confirmation message. After user submits selections, returns list of tracked items.

#### `get_pending_selections`

Check if the user has submitted selections from an open report. Used if the conversation continues before the user submits.

**Returns:** List of selected listing IDs, or "no selections yet".

---

## MCP Tools

### `search_classifieds`

Search general classifieds listings. Returns structured data (for conversational use).

**Parameters:**
```json
{
  "keyword": "string (required)",
  "category": "string (optional) — category name or ID",
  "sub_category": "string (optional)",
  "price_from": "number (optional)",
  "price_to": "number (optional)",
  "zip": "string (optional)",
  "miles": "number (optional) — radius",
  "seller_type": "string (optional) — 'Private' | 'Business'",
  "market_type": "string (optional) — 'Sale' | 'Wanted' | 'Rent' | 'Service'",
  "has_photos": "boolean (optional)",
  "condition": "string (optional) — 'New' | 'Used'",
  "sort": "string (optional) — 'newest' | 'price_low' | 'price_high'",
  "page": "number (optional, default: 0)"
}
```

**Returns:** Array of listing summaries (title, price, location, image_url, listing_url, listing_id, favorites_count)

---

### `search_cars`

Search KSL Cars via the JSON API.

**Parameters:**
```json
{
  "keyword": "string (optional)",
  "make": "string (optional) — semicolon-separated for multiple",
  "model": "string (optional) — semicolon-separated for multiple",
  "year_from": "number (optional)",
  "year_to": "number (optional)",
  "price_from": "number (optional)",
  "price_to": "number (optional)",
  "mileage_to": "number (optional)",
  "zip": "string (optional)",
  "miles": "number (optional)",
  "title_type": "string (optional) — 'Clean Title'",
  "drive": "string (optional) — '4-Wheel Drive' | 'AWD' | 'FWD' | 'RWD'",
  "fuel": "string (optional) — 'Gasoline' | 'Diesel' | 'Electric' | 'Hybrid'",
  "page": "number (optional, default: 1)"
}
```

**Returns:** Array of car listings (title, price, make, model, year, mileage, city, state, photo_url, listing_url, listing_id)

---

### `get_listing`

Get full details for a specific listing, including photos.

**Parameters:**
```json
{
  "listing_id": "string (required)",
  "platform": "string (optional) — 'classifieds' | 'cars', default: 'classifieds'"
}
```

**Returns:** Full listing detail (title, description, price, photos[] as base64 thumbnails, location, seller_type, posted_date, listing_url, category, condition)

---

### `track_item`

Add an item to the watch list for price tracking.

**Parameters:**
```json
{
  "listing_id": "string (required)",
  "platform": "string (optional) — 'classifieds' | 'cars'",
  "notes": "string (optional) — personal notes about why tracking"
}
```

**Returns:** Confirmation with current price snapshot saved.

---

### `untrack_item`

Remove an item from the watch list.

**Parameters:**
```json
{
  "listing_id": "string (required)"
}
```

---

### `mark_as_sold`

Manually mark a tracked item as sold (for cases where auto-detection isn't clear).

**Parameters:**
```json
{
  "listing_id": "string (required)",
  "sold_price": "number (optional) — final sale price if known"
}
```

---

### `list_tracked_items`

List all items currently being tracked.

**Parameters:**
```json
{
  "status": "string (optional) — 'active' | 'sold' | 'removed' | 'all'"
}
```

**Returns:** Array of tracked items with current price, original price, price change, days listed, last checked timestamp.

---

### `get_price_history`

Get price change history for a tracked item.

**Parameters:**
```json
{
  "listing_id": "string (required)"
}
```

**Returns:** Array of price snapshots (price, timestamp, change_from_previous).

---

### `research_item`

Given a link or description of a desired item, fetch the URL, extract product details (name, brand, model number, key specs, reference images), and search KSL for matching listings. Builds enough context to recognize the item from an image or description.

**Parameters:**
```json
{
  "url": "string (optional) — link to a product page (Amazon, manufacturer, etc.)",
  "description": "string (optional) — natural language description of what to find",
  "max_results": "number (optional, default: 10)"
}
```

**Returns:** 
- Extracted product profile (name, brand, model, specs, reference image URLs)
- Array of potential KSL matches with listing details and prices

**Implementation:**
1. Fetch the provided URL, extract product metadata (title, brand, model, images, specs)
2. Generate multiple search queries (brand + model, common abbreviations, category keywords)
3. Run searches against KSL with rate limiting
4. Deduplicate and return results with the reference product profile for comparison

---

### `list_categories`

List available categories and subcategories.

**Parameters:**
```json
{
  "parent_category": "string (optional) — if provided, list subcategories"
}
```

**Returns:** Array of categories with IDs and names.

---

## Sold Detection

Items are detected as sold/removed through two mechanisms:

1. **Listing shows "sold" indicator** — KSL overlays "SOLD" on the listing image. When checking a tracked item, if the page contains a sold indicator → mark status as `sold`, record final price snapshot.

2. **Listing returns 404/gone** — The listing has been removed entirely → mark status as `removed` (likely sold or expired).

3. **Manual override** — `mark_as_sold` tool for cases where the user knows an item sold (e.g., they bought it).

Both auto-detection cases trigger a final price snapshot recording.

---

## Data Models

### Listing

```sql
CREATE TABLE listings (
  id TEXT PRIMARY KEY,          -- KSL listing ID
  platform TEXT NOT NULL,       -- 'classifieds' | 'cars'
  title TEXT NOT NULL,
  description TEXT,
  price REAL,
  photos TEXT,                  -- JSON array of URLs
  city TEXT,
  state TEXT,
  zip TEXT,
  category TEXT,
  sub_category TEXT,
  seller_type TEXT,
  posted_date TEXT,
  url TEXT NOT NULL,
  favorites_count INTEGER,
  -- cars-specific
  make TEXT,
  model TEXT,
  year INTEGER,
  mileage INTEGER,
  -- metadata
  first_seen_at TEXT NOT NULL,
  last_fetched_at TEXT NOT NULL
);
```

### TrackedItem

```sql
CREATE TABLE tracked_items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  listing_id TEXT NOT NULL REFERENCES listings(id),
  platform TEXT NOT NULL,
  notes TEXT,
  first_seen_price REAL,
  current_price REAL,
  first_seen_at TEXT NOT NULL,
  last_checked_at TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',  -- 'active' | 'sold' | 'removed' | 'expired'
  sold_price REAL,
  UNIQUE(listing_id)
);
```

### PriceSnapshot

```sql
CREATE TABLE price_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  listing_id TEXT NOT NULL REFERENCES listings(id),
  price REAL NOT NULL,
  recorded_at TEXT NOT NULL
);
```

### SavedSearch (Phase 2)

```sql
CREATE TABLE saved_searches (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  parameters TEXT NOT NULL,     -- JSON
  platform TEXT NOT NULL,
  schedule TEXT,                -- cron expression
  last_run_at TEXT,
  created_at TEXT NOT NULL
);
```

---

## Sales Pattern Analysis

### Approach

The MCP server focuses on **data collection and storage**. Pattern analysis is performed by the LLM using the collected data via tools.

### Data Collected (MVP)

- Duration listed (first_seen_at → sold/removed timestamp)
- Price change history (number of drops, magnitude, timing)
- Listing metadata (category, photo count, description length, seller type, favorites count)

### Future: `get_sales_stats` Tool (Phase 2)

Aggregate queries the LLM can request:
- Average days-to-sell by category
- Price drop frequency before sale
- Listings with price significantly below category average

### Out of Scope (LLM's Job)

- Image quality analysis (LLM can view photos via `get_listing`)
- Description effectiveness analysis (LLM reads descriptions directly)
- Pattern recognition and recommendations (conversational)

Future: Local GPU-accelerated ML models for report generation (separate phase).

---

## Configuration

Stored in `~/.config/ksl-mcp/config.toml`:

```toml
[database]
path = "~/.local/share/ksl-mcp/ksl.db"

[rate_limit]
min_delay_ms = 3000
max_delay_ms = 8000
max_daily_requests = 500

[scraping]
user_agent = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2.1 Safari/605.1.15"

[report]
open_browser = true
timeout_seconds = 600  # 10 minutes
```

---

## Distribution

```bash
# Install from source
cargo install --git https://github.com/chriswessells/ksl-classifieds-mcp

# Or from crates.io (future)
cargo install ksl-classifieds-mcp
```

### MCP Client Configuration (Kiro/Claude)

```json
{
  "mcpServers": {
    "ksl-classifieds": {
      "command": "ksl-classifieds-mcp",
      "args": [],
      "env": {}
    }
  }
}
```

---

## Phase 2 Features (Next)

- **Cron/scheduler**: Background process that polls tracked items and saved searches on a schedule
- **Saved searches**: Save search parameters, run on schedule, detect new/changed listings
- **Alerts**: Configurable notifications when:
  - A tracked item's price drops
  - A new listing matches a saved search
  - A tracked item is removed (likely sold)
- **Export**: Export tracked data as CSV/JSON for external analysis
- **`get_sales_stats`**: Aggregate statistics tool for pattern analysis

---

## Resolved Decisions

| Question | Decision |
|----------|----------|
| Database | SQLite — relational model fits, small binary, battle-tested |
| `research_item` behavior | Fetches URL, extracts full product profile (name, brand, model, specs, images), generates multiple search queries | 
| Sold detection | Auto-detect via "sold" indicator on page OR 404 response, plus manual `mark_as_sold` tool |
| Result presentation | Interactive HTML report with photos + checkboxes, served via temporary local HTTP server |
| Pattern analysis | Server collects data, LLM does the analysis. Future: local GPU ML models for reports |
| Binary size | SQLite keeps it small (~5-10MB total binary) |
