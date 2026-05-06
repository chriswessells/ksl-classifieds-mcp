# Stage 1 DAG — Core Search

## Visual DAG

```
Phase 1A: Foundation
═══════════════════════════════════════════════════════════════

  ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐
  │  1A.1   │   │  1A.2   │   │  1A.3   │   │  1A.4   │
  │ Errors  │   │ Types   │   │ Config  │   │ Categs  │
  └────┬────┘   └────┬────┘   └────┬────┘   └────┬────┘
       │              │             │              │
       │              │             │         (needs 1A.2)
       │              │             │              │
       └──────┬───────┘             │              │
              │                     │              │
              ▼                     │              │
        ┌─────────┐                │              │
        │  1A.5   │◄───────────────┘              │
        │ Module  │◄──────────────────────────────┘
        │Structure│
        └────┬────┘
             │
═════════════╪═════════════════════════════════════════════════
             │
Phase 1B & 1C: HTTP Infrastructure + HTML Parsing (PARALLEL)
═══════════════════════════════════════════════════════════════

             ├──────────────────────────┐
             │                          │
             ▼                          ▼
       ┌──────────┐              ┌──────────┐
       │   1B.1   │              │   1C.1   │
       │   Rate   │              │  Parser  │
       │  Limiter │              │  (HTML)  │
       └────┬─────┘              └────┬─────┘
            │                         │
            ▼                         │
       ┌──────────┐                   │
       │   1B.2   │◄──────────────────┘
       │   HTTP   │
       │  Client  │
       └────┬─────┘
            │
═══════════╪══════════════════════════════════════════════════
            │
Phase 1D: MCP Integration
═══════════════════════════════════════════════════════════════

            ▼
       ┌──────────┐
       │   1D.1   │
       │   MCP    │
       │  Server  │
       │ + Tools  │
       └──────────┘
```

## Parallelization Matrix

| Phase | Work Items | Parallel? | Notes |
|-------|-----------|-----------|-------|
| 1A | 1A.1, 1A.2, 1A.3 | ✅ All parallel | No interdependencies |
| 1A | 1A.4 | ⚠️ After 1A.2 | Needs `Category` type |
| 1A | 1A.5 | ⚠️ After 1A.1 + 1A.2 | Declares modules, defines trait |
| 1B | 1B.1 | ✅ After 1A | Needs config + error types |
| 1C | 1C.1 | ✅ After 1A | Needs types + error types |
| 1B | 1B.2 | ⚠️ After 1B.1 + 1C.1 | Needs rate limiter + parser |
| 1D | 1D.1 | ❌ After all | Integrates everything |

## Dependency Graph (Flat)

```
1A.1 (Errors)         → no deps
1A.2 (Types)          → no deps
1A.3 (Config)         → no deps
1A.4 (Categories)     → 1A.2
1A.5 (Module Struct)  → 1A.1, 1A.2, 1A.3
1B.1 (Rate Limiter)   → 1A.1, 1A.3
1C.1 (Parser)         → 1A.1, 1A.2
1B.2 (HTTP Client)    → 1A.5, 1B.1, 1C.1
1D.1 (MCP Server)     → 1A.4, 1B.2
```

## Execution Order (Critical Path)

```
Step 1 (parallel):  1A.1, 1A.2, 1A.3
Step 2 (parallel):  1A.4, 1A.5
Step 3 (parallel):  1B.1, 1C.1
Step 4 (serial):    1B.2
Step 5 (serial):    1D.1
```

**Critical path:** 1A.2 → 1A.5 → 1C.1 → 1B.2 → 1D.1

## Estimated Effort

| Work Item | Estimate | Risk |
|-----------|----------|------|
| 1A.1 Errors | Small | Low |
| 1A.2 Types | Small | Low |
| 1A.3 Config | Medium | Low — straightforward TOML parsing |
| 1A.4 Categories | Small | Low — pure data |
| 1A.5 Module Structure | Small | Medium — rmcp API surface may surprise |
| 1B.1 Rate Limiter | Medium | Medium — async timing logic |
| 1B.2 HTTP Client | Medium | Low — straightforward once deps ready |
| 1C.1 Parser | Medium | High — CSS selectors depend on live HTML structure |
| 1D.1 MCP Server | Large | Medium — rmcp integration, tool schema |

## Risk Notes

1. **Parser (1C.1)** is highest risk. KSL's HTML structure may differ from what's documented. Mitigation: capture a real page early, build fixture from it.
2. **rmcp API (1D.1)** — the rmcp crate's exact API for tool registration may require adaptation. Mitigation: read rmcp examples/docs before starting 1A.5.
3. **Rate limiter timing tests (1B.1)** — async timing tests can be flaky. Mitigation: use `tokio::time::pause()` for deterministic tests.

## Done Criteria for Stage 1

Stage 1 is complete when:
- [ ] `cargo build` produces a binary
- [ ] `cargo test` passes all unit tests
- [ ] Binary starts and completes MCP stdio handshake
- [ ] `list_categories` returns 29 categories
- [ ] `search_classifieds` with a keyword returns structured listings from live KSL
- [ ] Rate limiter enforces delays between requests
- [ ] Missing config file → server starts normally with defaults
- [ ] Daily cap exceeded → returns clear error message to MCP client
