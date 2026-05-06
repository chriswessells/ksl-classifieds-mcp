# Roadmap

## Done

- [x] Define spec (tools, data models, storage)
- [x] Build MVP with stdio transport (local use)
- [x] KSL Classifieds search via HTML scraping
- [x] KSL Cars search via JSON API
- [x] Listing detail fetching
- [x] Price tracking with SQLite persistence
- [x] Interactive HTML report browser with CSRF protection
- [x] Saved searches (save, list, delete, run)
- [x] Aggregate sales statistics
- [x] Rate limiting (per-endpoint backoff, daily cap)
- [x] GitHub Actions CI (clippy + tests)
- [x] Release workflow (macOS arm64 + Linux x64 binaries)

## Next

- [ ] Background price polling via cron/Lambda
- [ ] Sold/removed auto-detection during background checks
- [ ] Push notifications (email, Discord)

## Later / Ideas

- [ ] Deploy to Bedrock AgentCore Runtime (streamable-HTTP transport)
- [ ] Price trend analysis / deal scoring
- [ ] Additional filter support (distance, seller type)
