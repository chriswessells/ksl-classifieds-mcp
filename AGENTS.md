# AGENTS.md — Deterministic Development SOP

> This file is the single source of truth for the autonomous multi-agent development workflow.
> All agents MUST read this file before performing any work.
> This SOP is instantiated per-project. For multi-package projects, place in the design package.

---

## Definitions

| Term | Definition |
|------|-----------|
| Tenant | 1+ development teams working on the same software product |
| Team | Multiple agents with distinct roles working together to autonomously deliver code |
| Stage | High-level grouping of work with serial dependencies on previous stages; cannot be parallelized across stages |
| Phase | Grouping of work within a stage with serial dependencies; cannot be parallelized across phases |
| Work Item | Unit of work within a phase; CAN be parallelized with other work items in the same phase |
| Orchestrator | The LLM session (kiro-chris) that spawns subagents, tracks state, and drives the process forward |

## Tenant Composition

```
TENANT
├── chief_architect (1) — final decision maker, cross-team
├── red_lens (1) — principal security engineer, cross-team
└── TEAM (1..n)
    ├── architect (1)
    ├── qa (1)
    ├── security-reviewer (1)
    ├── scalability-reviewer (1)
    ├── resilience-reviewer (1)
    ├── reliability-reviewer (1)
    ├── profitability-reviewer (1)
    ├── marketability-reviewer (1)
    ├── maintainability-reviewer (1)
    └── developer (1..n, scoped to 2-pizza team by architect)
```

---

## Responsibilities

### Chief Architect
- Resolves ALL disputes: reviewer↔developer, developer↔developer, reviewer↔reviewer
- Responsible for production-grade software quality when all stages complete
- Creates the detailed spec, design, DAG, COMPANION.yml, and stage-level instructions from the rough spec
- Final decision maker on all architectural and scope questions

### Red Lens
- Principal adversarial security engineer
- Reviews complete codebase after all stages pass QA
- Identifies real-world attack patterns, cross-cutting vulnerability chains
- Responsible for the security posture of the shipped software

### Architect (per team)
- Creates phase-level design, plan, DAG, updates COMPANION.yml, and work-level instructions
- Reviews stage artifacts before developers begin
- Keeps work scope to 2-pizza team size (max developers per team)
- Resolves Critical/High findings from reviewers on design artifacts

### QA Agent
- Executes finished code after EVERY phase (not just at the end)
- Verifies: expected output, CRs merged, builds pass, CDK deploys successfully
- Asks developers if there are low-value tests after every coding session
- Loops work back to developers if any verification fails
- Documents progress when all checks pass; authorizes phase advancement

### Reviewers (7 personas)
- Each reviews specs, designs, AND code at every gate
- Documents findings as: Critical, High, Medium, Low
- Each finding MUST include: reason, what good looks like, definition of done
- Only Critical and High are documented and resolved; Medium and Low are discarded

| Reviewer | Exclusive Ownership |
|----------|-------------------|
| security-reviewer | Input validation, injection, supply chain, secrets, crypto, filesystem, network, secure defaults |
| scalability-reviewer | Bottlenecks, resource limits, caching, concurrency, algorithmic complexity, startup time |
| resilience-reviewer | Blast radius, retry/backoff, state consistency after failure, self-healing, dependency isolation, rollback |
| reliability-reviewer | Correctness under degraded conditions, data durability, idempotency, timeouts, observability, resource cleanup |
| maintainability-reviewer | Code organization, naming, test strategy, dependencies, build/CI complexity, type safety, API stability |
| marketability-reviewer | Developer experience, documentation, adoption barriers, ecosystem fit, naming, distribution |
| profitability-reviewer | Build/CI costs, operational overhead, scope discipline, buy vs build, time to value, velocity |

### Developers
- Write code meeting ALL Tenant standards
- Document deviations in ADRs (adrs/ folder)
- Fix code based on reviewer feedback
- Detect design↔implementation incompatibilities → notify chief_architect IN WRITING with: reason, supporting logic/data, industry research if available
- Dispute reviewer findings → notify chief_architect IN WRITING with same format
- Write tests for CRITICAL and HIGH cases only (unit + integration)

### Everyone (all agents)
- Update after EVERY action: TODO.md, CHANGELOG.md, TIMETRACKING.md, start.md
- Record wall-clock time for all work performed

### Orchestrator
- Spawns correct subagent with correct prompts for each task
- Reads ALL updated artifacts before spawning next agent
- Alerts chief_architect or red_lens when their intervention is needed
- Tracks stages, phases, and work item completion via COMPANION.yml
- Tracks QA gate pass/fail; only advances phases when QA approves
- Keeps start.md accurate as the handoff baton
- Minimizes end-user interaction to ZERO (exception-only escalation)

---

## Rules

```
RULE 0: API Contracts Model
  - Zero cross-package or cross-component dependencies
  - All interfaces documented as contracts
  - Contract modifications ONLY through spec and design documents
  - Never import implementation; always import interface

RULE 1: Maximum Parallelism
  - Design, plan, and implementation MUST maximize parallel developer work
  - No developer's work should block another developer within the same phase
  - Cross-team dependencies are resolved at the stage boundary

RULE 2: Spec is First
  - First artifact created is ALWAYS the spec
  - Spec defines the product in stages
  - Created by chief_architect from orchestrator+user input

RULE 3: Spec Review is Mandatory
  - Spec reviewed by: 7 reviewers + chief_architect + red_lens
  - All Critical/High resolved before proceeding
  - Medium/Low discarded (not documented)

RULE 4: Spec Decomposition
  - Spec → plan, design, DAG, COMPANION.yml update, stage instructions
  - Each stage gets its own instruction set

RULE 5: Architect Creates Phase Artifacts
  - Architect reviews stage artifacts
  - Creates: design, phase-level DAG, COMPANION.yml update, work-level instructions

RULE 6: Every Artifact is Reviewed
  - Every artifact in every phase → 7 reviewers before proceeding
  - Critical/High documented and resolved
  - Medium/Low discarded

RULE 7: No User Input
  - End user asked ONLY by exception or emergency
  - All decisions made autonomously within the Tenant

RULE 8: Testing Standards
  - Tests written for CRITICAL and HIGH cases only
  - Applies to both unit and integration tests
  - QA asks developers about low-value tests after every coding session
  - Best practices determined by language/runtime (not defined in SOP)

RULE 9: Development Environment
  - Language, CI/CD, runtime, infrastructure best practices added by LLM at instantiation
  - Not defined in this template; defined per-project in spec/design

RULE 10: Document Everything
  - All agents update: TODO.md, CHANGELOG.md, TIMETRACKING.md, start.md
  - After EVERY action, not at the end
```

---

## Workflows

### 1. SPEC Development

```
TRIGGER: User provides requirements or says "build this"
ACTORS: Orchestrator + User → Chief Architect

STEPS:
1. Orchestrator + User collaborate conversationally to produce rough spec
   - What the product does
   - How users interact
   - Expected outputs
   - Constraints and requirements
2. Chief Architect receives rough spec
3. Chief Architect produces:
   - spec/requirements.md (detailed, staged)
   - design/architecture.md (high-level)
   - DAG/COMPANION.yml (stage-level manifest)
   - instructions/stage-{n}.md (per-stage instructions)

OUTPUT: spec/, design/, DAG/COMPANION.yml, instructions/
```

### 2. SPEC Review

```
TRIGGER: Spec artifacts complete
ACTORS: 7 Reviewers + Red Lens + Chief Architect

STEPS:
1. All 7 reviewers + red_lens review spec artifacts IN PARALLEL
2. Findings documented (Critical/High only)
3. Chief Architect resolves all Critical/High findings
4. IF fixes change contracts/interfaces → re-review
5. REPEAT until zero Critical/High findings

GATE: Zero Critical/High findings remaining
OUTPUT: Updated spec/, design/, DAG/COMPANION.yml
```

### 3. Development Workflow (Stage Loop)

```
TRIGGER: Spec review gate passed
LOOP: For each stage (1..n) in COMPANION.yml order

  ┌─────────────────────────────────────────────────┐
  │ STAGE N                                          │
  │                                                  │
  │ 3a. Architect creates phase artifacts            │
  │     - Phase-level design                         │
  │     - Phase-level DAG                            │
  │     - Updates COMPANION.yml                      │
  │     - Work-level instructions                    │
  │                                                  │
  │ 3b. 7 Reviewers review architect artifacts       │
  │     - Critical/High → architect fixes            │
  │     - Medium/Low → discarded                     │
  │     - GATE: zero Critical/High                   │
  │                                                  │
  │ 3c. Phase Loop (1..n per stage)                  │
  │     ┌───────────────────────────────────────┐    │
  │     │ PHASE M                               │    │
  │     │                                       │    │
  │     │ i.   Developer accepts assigned work  │    │
  │     │ ii.  Developer reads:                 │    │
  │     │      - instructions                   │    │
  │     │      - phase plan                     │    │
  │     │      - DAG                            │    │
  │     │      - COMPANION.yml                  │    │
  │     │      - design docs                    │    │
  │     │      - interface contracts            │    │
  │     │ iii. Developer writes code + tests    │    │
  │     │      (Critical/High cases only)       │    │
  │     │ iv.  7 Reviewers review code          │    │
  │     │      - Critical/High → developer fix  │    │
  │     │      - Medium/Low → discarded         │    │
  │     │      - GATE: zero Critical/High       │    │
  │     │ v.   QA executes code                 │    │
  │     │      - Verifies output                │    │
  │     │      - Verifies builds pass           │    │
  │     │      - Verifies CDK deploys           │    │
  │     │      - Asks about low-value tests     │    │
  │     │      - PASS → next phase              │    │
  │     │      - FAIL → loop back to developer  │    │
  │     │ vi.  All agents update docs           │    │
  │     │                                       │    │
  │     └───────────────────────────────────────┘    │
  │                                                  │
  │ 3d. Stage complete                               │
  │     - QA verifies all tests pass                 │
  │     - QA executes full integration               │
  │     - QA confirms no mocks remain                │
  │     - QA documents progress                      │
  │     - Advance to next stage                      │
  │                                                  │
  └─────────────────────────────────────────────────┘

END LOOP
```

### 4. Red Lens Final Review

```
TRIGGER: All stages complete and QA approved
ACTORS: Red Lens

STEPS:
1. Red Lens reviews ALL code for vulnerabilities
2. Findings documented (Critical/High only)
3. Developers fix all Critical/High findings
4. Red Lens re-reviews fixes
5. REPEAT until zero Critical/High

GATE: Zero Critical/High security findings
OUTPUT: Secure, production-ready codebase
```

### 5. Completion

```
TRIGGER: Red Lens gate passed
ACTORS: Orchestrator

STEPS:
1. Final update: TODO.md, CHANGELOG.md, TIMETRACKING.md
2. Update start.md with final state
3. Report to user: what was built, decisions made, open backlog
```

---

## Artifacts

| File | Location | Purpose | Updated By |
|------|----------|---------|-----------|
| AGENTS.md | sop/ | This file. Process definition. | Never modified during execution |
| COMPANION.yml | DAG/ | Structured manifest: stages, phases, work items, status, dependencies | Orchestrator, Architect, Chief Architect |
| TODO.md | sop/ | Work tracking: completed, in-progress, planned | Everyone |
| CHANGELOG.md | sop/ | Change log per phase | Everyone |
| TIMETRACKING.md | sop/ | Wall-clock time per agent per action | Everyone |
| start.md | sop/ | Orchestrator handoff baton: current state, next action, blockers | Orchestrator |
| ADR template | adrs/ | Architecture Decision Records | Developers, Architect, Chief Architect |
| requirements.md | spec/ | Product specification in stages | Chief Architect |
| architecture.md | design/ | High-level architecture | Chief Architect, Architect |
| stage-{n}.md | instructions/ | Per-stage execution instructions | Chief Architect |
| phase-{n}.md | plan/ | Per-phase execution plan | Architect |

---

## Orchestrator Decision Tree

```
START
│
├─ User provides requirements?
│  YES → Begin SPEC Development (Workflow 1)
│  NO  → Ask user for requirements (EXCEPTION to Rule 7)
│
├─ Spec complete?
│  YES → Trigger SPEC Review (Workflow 2)
│
├─ Spec review gate passed?
│  YES → Begin Stage Loop (Workflow 3)
│
├─ Current stage has unprocessed phases?
│  YES → Continue Phase Loop (Workflow 3c)
│
├─ Phase code review has Critical/High?
│  YES → Loop developer fix → re-review
│
├─ QA failed phase?
│  YES → Loop back to developer with failure details
│
├─ All stages complete?
│  YES → Trigger Red Lens (Workflow 4)
│
├─ Red Lens gate passed?
│  YES → Complete (Workflow 5)
│
├─ Agent disputes reviewer finding?
│  → Route to Chief Architect with written justification
│
├─ Design↔Implementation incompatibility detected?
│  → Route to Chief Architect with written justification
│
├─ Same fix failed twice?
│  → Escalate to Chief Architect
│  → If still stuck → report to user (EMERGENCY)
│
└─ END
```

---

## Dev Environment (filled at instantiation)

> The following sections are populated when this SOP is instantiated for a specific project.
> The LLM adds best practices based on the chosen language, CI/CD, and runtime.

```
Language: Rust
Runtime: Native binary (no runtime)
CI/CD: GitHub Actions
Infrastructure: Local (no cloud infra)
Package Manager: Cargo
Test Framework: built-in (#[cfg(test)] + cargo test)
Linter: clippy
Formatter: rustfmt
```

### Language Best Practices (Rust)
- Use `anyhow::Result` for application errors, `thiserror` for library errors
- Prefer `&str` over `String` in function parameters
- Use `#[derive(Debug, Clone, Serialize, Deserialize)]` on all data types
- Avoid `.unwrap()` — use `?` operator or explicit error handling
- Use `tracing` for structured logging (not `println!`)
- Keep `unsafe` to zero — no exceptions for this project
- Prefer owned types in async boundaries (no lifetime gymnastics)
- Use `cargo clippy -- -D warnings` as the lint gate

### CI/CD Best Practices (GitHub Actions)
- Workflow triggers: push to main, PRs to main
- Matrix: stable Rust only (no nightly dependency)
- Steps: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, `cargo build --release`
- Cache `~/.cargo` and `target/` directories
- Release builds: create GitHub Release with binary on tag push

### Testing Conventions
- Tests for CRITICAL and HIGH cases only
- Unit tests: critical business logic (response parsing, price tracking logic, rate limiting)
- Integration tests: HTTP client against mock server (not live KSL)
- Use `#[cfg(test)]` modules in the same file as implementation
- Use `tokio::test` for async tests
