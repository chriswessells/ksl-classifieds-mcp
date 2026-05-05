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
| Database | **SurrealDB embedded** | See [Database Decision](#database-decision) below |
| Distribution | `cargo install` from git or crates.io | Single binary, no runtime deps |

### Database Decision

| Option | Pros | Cons | Verdict |
|--------|------|------|---------|
| **SQLite** (`rusqlite`) | Battle-tested, SQL queries, great tooling, tiny footprint | Schema migrations, no native graph/document model | Good default |
| **SurrealDB embedded** | Multi-model (document + graph + relational), SurrealQL, native Rust, schema-flexible, relationships between items/searches/price-history are first-class | Larger binary, younger project, heavier dependency | **Selected** — the relationship modeling (item → price_history, search → items, item → similar_items) maps naturally to SurrealDB's graph capabilities. Price tracking over time and pattern analysis benefit from its flexible schema. |
| **redb** | Pure Rust, fast KV, ACID | No query language, manual indexing | Too low-level for our query needs |
| **sled** | Pure Rust, embedded | Unstable API, no SQL, maintenance concerns | Not recommended |

SurrealDB can run fully embedded (in-process, file-backed via RocksDB) with no external server. If SurrealDB proves too heavy during implementation, fallback to SQLite with `rusqlite` + manual schema.

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

## MCP Tools

### `search_classifieds`

Search general classifieds listings.

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

Get full details for a specific listing.

**Parameters:**
```json
{
  "listing_id": "string (required)",
  "platform": "string (optional) — 'classifieds' | 'cars', default: 'classifieds'"
}
```

**Returns:** Full listing detail (title, description, price, photos[], location, seller_type, posted_date, listing_url, category, condition)

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

Given a link or description of a desired item, search KSL to find matching listings. Handles cases where KSL listing titles/descriptions don't match standard product names.

**Parameters:**
```json
{
  "url": "string (optional) — link to a product page (Amazon, manufacturer, etc.)",
  "description": "string (optional) — natural language description of what to find",
  "max_results": "number (optional, default: 10)"
}
```

**Returns:** Array of potential matches with relevance reasoning, listing details, and prices.

**Implementation notes:** This tool generates multiple search queries from the input (brand names, model numbers, common abbreviations, category keywords) and deduplicates results. The LLM calling this tool can then evaluate which results are actual matches.

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

## Data Models

### Listing

```
listing {
  id: string,
  platform: 'classifieds' | 'cars',
  title: string,
  description: string,
  price: number | null,
  photos: [string],
  location: { city: string, state: string, zip: string? },
  category: string,
  sub_category: string?,
  seller_type: string,
  posted_date: datetime?,
  url: string,
  favorites_count: number?,
  // cars-specific
  make: string?,
  model: string?,
  year: number?,
  mileage: number?,
}
```

### TrackedItem

```
tracked_item {
  id: string (auto),
  listing_id: string,
  platform: string,
  notes: string?,
  first_seen_price: number,
  current_price: number,
  first_seen_at: datetime,
  last_checked_at: datetime,
  status: 'active' | 'sold' | 'removed' | 'expired',
  -> price_history: [PriceSnapshot],
}
```

### PriceSnapshot

```
price_snapshot {
  id: string (auto),
  tracked_item: reference,
  price: number,
  recorded_at: datetime,
}
```

### SavedSearch (Phase 2)

```
saved_search {
  id: string (auto),
  name: string,
  parameters: json,
  platform: string,
  schedule: string?, // cron expression
  last_run_at: datetime?,
  created_at: datetime,
}
```

---

## Sales Pattern Analysis — Scope Discussion

### In Scope (MVP)

- Track how long items are listed before being sold/removed
- Record price changes over time (how many drops before sale)
- Store listing metadata (category, photos count, description length, seller type)

### In Scope (Phase 2)

- Aggregate statistics: average days-to-sell by category
- Price drop patterns: "items that drop 20%+ in first week sell within 3 days"
- Identify underpriced listings (price significantly below category average)

### Potentially Out of Scope / Needs Discussion

- **Image analysis** — Identifying photo quality/style patterns that correlate with faster sales. This requires ML/vision capabilities beyond the MCP server itself. Could be a tool that returns image URLs for the LLM to analyze.
- **Description NLP** — Analyzing which description styles/keywords correlate with sales. Same as above — the LLM calling the tools can do this analysis given the raw data.
- **Seller reputation scoring** — Tracking seller history across listings. Requires identifying sellers across listings (member ID scraping).
- **Market timing** — Day-of-week/time-of-day posting patterns. Requires posted_date precision which may not be available.

**Recommendation:** The MCP server should focus on **data collection and storage**. The pattern analysis itself is best done by the LLM using the collected data via the tools. The server provides `get_price_history`, `list_tracked_items` (with duration/price data), and a future `get_sales_stats` tool. The intelligence layer (pattern recognition, recommendations) lives in the LLM conversation, not the server.

---

## Configuration

Stored in `~/.config/ksl-mcp/config.toml`:

```toml
[database]
path = "~/.local/share/ksl-mcp/data"  # SurrealDB file store

[rate_limit]
min_delay_ms = 3000
max_delay_ms = 8000
max_daily_requests = 500

[scraping]
user_agent = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) ..."
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

---

## Open Questions

1. Should `research_item` attempt to parse external URLs (Amazon, etc.) to extract product details, or just pass the URL/description to the search as-is?
2. For sales pattern analysis, do we need a `mark_as_sold` tool for manual confirmation, or rely on detecting listing removal?
3. SurrealDB embedded adds ~20MB to binary size — acceptable?
