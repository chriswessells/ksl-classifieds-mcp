# start.md — Orchestrator Handoff Baton

## Current State

```
Project: ksl-classifieds-mcp
Status: COMPLETE
All Stages: passed
Red Lens: passed (zero Critical/High)
Tests: 35 passing
Clippy: clean
Timestamp: 2026-05-05T21:30:00Z
```

## What Was Built

A Rust MCP server for KSL Classifieds with 12 tools:

| Tool | Stage | Description |
|------|-------|-------------|
| search_classifieds | 1 | Search KSL Classifieds via HTML scraping |
| list_categories | 1 | 29 hardcoded categories |
| get_listing | 2 | Fetch listing detail page |
| search_cars | 2 | Search KSL Cars via JSON API |
| track_item | 3 | Track a listing for price changes |
| untrack_item | 3 | Stop tracking |
| list_tracked_items | 3 | List all tracked items |
| get_price_history | 3 | Price snapshots over time |
| mark_as_sold | 3 | Manual sold marking |
| browse_search_results | 4 | Visual HTML report in browser |
| get_pending_selections | 4 | Check user selections from report |
| save_search | 5 | Save search parameters |
| list_saved_searches | 5 | List saved searches |
| delete_saved_search | 5 | Delete a saved search |
| run_saved_search | 5 | Re-run a saved search |
| get_sales_stats | 5 | Aggregate statistics |

## Architecture

- Rust, single binary (~7MB release)
- rmcp MCP SDK, stdio transport
- reqwest HTTP client with rate limiting (3-8s delay, 500/day cap, per-endpoint backoff)
- scraper for HTML parsing
- rusqlite (WAL mode) for persistence
- axum singleton report server (127.0.0.1:0)

## Security Posture

- All SQL parameterized (rusqlite params![])
- HTML output escaped (5-char escape + CSP script-src 'none')
- CSRF: 128-bit OsRng token, hidden field, server-side validation
- Localhost-only report server
- Hardcoded target domains (no SSRF surface)
- 2MB response size cap
- data_dir validated within $HOME
- TLS validation enforced

## Open Backlog (Roadmap)

- Background polling/cron for tracked items
- Push notifications (email/Discord)
- Sold/removed auto-detection during background checks
- GitHub Actions CI workflow
- Pre-built binary releases
- Bedrock AgentCore deployment (future)
