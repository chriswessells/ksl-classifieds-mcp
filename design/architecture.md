# Architecture — ksl-classifieds-mcp

## Component Diagram

```
┌─────────────────────────────────────────────────────────┐
│ MCP Client (Kiro/Claude)                                │
└──────────────────────────┬──────────────────────────────┘
                           │ stdio (JSON-RPC)
┌──────────────────────────▼──────────────────────────────┐
│ ksl-classifieds-mcp binary                              │
│                                                         │
│  ┌─────────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │ MCP Server  │  │ Tool Router  │  │ Report Server │  │
│  │ (rmcp)      │──│ (tools/mod)  │──│ (axum,        │  │
│  └─────────────┘  └──────┬───────┘  │  singleton)   │  │
│                           │          └───────────────┘  │
│  ┌────────────────────────▼─────────────────────────┐   │
│  │ Service Layer                                    │   │
│  │  ┌──────────┐ ┌──────────┐ ┌─────────────────┐  │   │
│  │  │ Search   │ │ Tracker  │ │ Stats           │  │   │
│  │  │ Service  │ │ Service  │ │ Service         │  │   │
│  │  └────┬─────┘ └────┬─────┘ └────┬────────────┘  │   │
│  └───────┼─────────────┼────────────┼───────────────┘   │
│          │             │            │                   │
│  ┌───────▼─────────────▼────────────▼───────────────┐   │
│  │ Infrastructure                                   │   │
│  │  ┌──────────────┐  ┌────────────────────────┐    │   │
│  │  │ KSL Client   │  │ SQLite (rusqlite, WAL) │    │   │
│  │  │ (reqwest)    │  │ ~/.local/share/ksl-mcp │    │   │
│  │  │ + RateLimiter│  └────────────────────────┘    │   │
│  │  │ (per-endpoint│                                │   │
│  │  │  backoff)    │                                │   │
│  │  └──────────────┘                                │   │
│  └──────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

## Module Structure

```
src/
├── main.rs              # Entry point, MCP server init, stdio transport, graceful degradation
├── tools/
│   ├── mod.rs           # Tool registration
│   ├── search.rs        # search_classifieds, search_cars, list_categories
│   ├── listing.rs       # get_listing
│   ├── tracking.rs      # track_item, untrack_item, list_tracked_items, get_price_history, mark_as_sold
│   ├── report.rs        # browse_search_results, get_pending_selections
│   └── searches.rs      # save_search, list_saved_searches, delete_saved_search, run_saved_search
├── client/
│   ├── mod.rs           # KSL client facade (trait HttpClient for testability)
│   ├── classifieds.rs   # HTML scraping client (GET URL pattern + scraper)
│   ├── cars.rs          # Cars JSON proxy client
│   ├── rate_limiter.rs  # Per-endpoint rate limiting, backoff, daily cap
│   └── types.rs         # Private wire-format types (RawClassifiedsItem, RawCarsItem)
├── db/
│   ├── mod.rs           # Database initialization, migrations, schema_version
│   ├── listings.rs      # Listing CRUD (parameterized queries only)
│   ├── tracking.rs      # Tracked items + price snapshots (transactional)
│   └── searches.rs      # Saved searches
├── report/
│   ├── mod.rs           # Singleton report server (axum)
│   └── templates/       # Askama templates (auto-escaping)
├── config.rs            # Config file loading with defaults fallback
└── types.rs             # Shared domain types (Platform, TrackingStatus, Listing, etc.)

tests/
├── fixtures/            # Captured KSL HTML/JSON responses
├── classifieds_parsing.rs
├── cars_parsing.rs
└── tracking_idempotency.rs
```

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| HTML scraping over RSC server actions (Stage 1) | GET URL pattern is stable, public-facing; server action hash changes on every deploy requiring fragile auto-recovery |
| SQLite WAL mode | Enables concurrent reads during writes; one-liner improvement |
| Per-endpoint backoff | A 429 on detail fetch must not block search operations |
| Singleton report server | Prevents fd exhaustion from multiple spawned servers; simpler lifecycle |
| `askama` for HTML templates | Auto-escaping by default prevents XSS from listing data |
| Wire-format isolation | KSL changes only affect `client/types.rs` + `From` impls, not the rest of the app |
| Typed enums for domain values | Compiler catches invalid states; no string matching |
| Graceful degradation on DB failure | Search tools work even if persistence layer is broken |

## Data Flow: Search

```
User → MCP Client → stdio → Tool Router → Search Service
  → KSL Client (rate limited, per-endpoint)
  → GET classifieds.ksl.com/v2/search/keyword/{kw}/...
  → Parse HTML with scraper (CSS selectors on <a role="listitem">)
  → Map RawClassifiedsItem → Listing (From impl)
  → Return structured listings to MCP Client
```

## Data Flow: Price Tracking

```
track_item:
  → fetch listing (rate limited)
  → BEGIN IMMEDIATE transaction:
      → INSERT OR IGNORE into listings
      → INSERT OR IGNORE into tracked_items (UNIQUE on listing_id)
      → INSERT into price_snapshots (skip if same price within 60s)
  → COMMIT
  → Return confirmation

price check (later):
  → fetch listing (rate limited)
  → BEGIN IMMEDIATE transaction:
      → INSERT price_snapshot (if price changed or >60s since last)
      → UPDATE tracked_items.current_price, last_checked_at
  → COMMIT
  → if HTTP 404 (confirmed with 2nd fetch) → mark removed in transaction
  → if sold indicator detected → mark sold in transaction
  → 5xx/connection error → log warning, do NOT change status
```

## Data Flow: Report

```
browse_search_results:
  → Run search (reuses search service)
  → Generate report HTML via askama (auto-escaped)
  → Register report at /report/{uuid} on singleton axum server
  → Open browser (platform-aware: open/xdg-open)
  → Return report URL to MCP client

User submits form:
  → POST /report/{uuid}/submit with CSRF token
  → Validate CSRF (128-bit, CSPRNG, single-use) → 403 on failure
  → Persist selections to pending_selections table (BEFORE 200 response)
  → Return 200 with confirmation page
  → get_pending_selections reads from pending_selections table
```

## Security Boundaries

| Boundary | Threat | Mitigation |
|----------|--------|-----------|
| KSL listing data → HTML report | XSS | askama auto-escaping + CSP `script-src 'none'` |
| KSL listing data → SQLite | SQL injection | Parameterized queries only (rusqlite params![]) |
| Report server on localhost | CSRF from other tabs | 128-bit CSPRNG token, hidden field, server-side validation |
| Action hash recovery | Untrusted JS parsing | Strict regex (exact length + charset), max 2 attempts, rate-limited |
| External URL fetch (future) | SSRF | Scheme allowlist (https), IP blocklist (private/loopback/link-local) |

## Configuration

```
~/.config/ksl-mcp/config.toml    # User config (fallback to defaults if missing)
~/.local/share/ksl-mcp/ksl.db    # SQLite database (WAL mode)
```

## Startup Sequence

1. Load config (or defaults if missing/malformed, log which)
2. Attempt DB initialization (create_dir_all, open, migrate schema)
3. If DB fails → log error, set `db: Option<Db>` to None (degraded mode)
4. Construct shared KSL client (single reqwest::Client with timeouts)
5. Construct rate limiter (per-endpoint state)
6. Register MCP tools (all tools registered; tracking tools check db availability)
7. Start stdio transport
8. Log startup: DB path, config path, schema version, mode (normal/degraded)
