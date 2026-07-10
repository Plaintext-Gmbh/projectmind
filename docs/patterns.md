# Pattern Lens

> **Status:** shipped in Cockpit Phase 2.3 ([#159](https://github.com/Plaintext-Gmbh/projectmind/issues/159)). Part of the Architect's Cockpit vision — the "what's slowly rotting" view.

Pattern Lens detects when a codebase **drifts away from its declared architectural patterns** and renders it as a compliance heatmap (rows = patterns, columns = modules). It is deliberately a bundle of cheap heuristics over the already-parsed model, not a full dataflow analysis.

## Detectors (v1, Java / Spring)

| Pattern | Id | Holds when… | Drift signal |
|---|---|---|---|
| **Repository** | `repository` | only `@Repository` classes touch `EntityManager` / `JdbcTemplate` directly | a `@Service` / `@Component` that owns an `EntityManager` field (or `@PersistenceContext`), or calls `entityManager.` / `getEntityManager()` / `jdbcTemplate.` in a method body |
| **Layered** | `layered` | `*.web` → `*.service` → `*.repository`, never backwards | a web/controller class references a repository or entity directly (field type or super-type). **v1 is intra-module only** — cross-module layering needs an inter-module relation pass (a known limitation) |
| **DI-only** | `di_only` | no manual `new XxxService()` inside a `@Component` | a `@Component`-typed class instantiates a bean (or a `*Service` / `*Repository` / `*Controller` / `*Component` type) with `new …(…)` |
| **@Tx-boundary** | `tx_on_service` | `@Transactional` only on `@Service` (or repository custom queries) | `@Transactional` on a `@Controller` / `@RestController` |
| **No-static-state** | `no_static_state` | no non-final `static` fields in a `@Component` | a non-final `static` field (a `static Map<…>` cache scores highest) |

The parse model captures classes, fields (type, `static`/`final`, annotations), methods and annotations, but **not method bodies**. The **Repository** and **DI-only** detectors therefore additionally read the class's source text within its line range — constructor calls and direct `EntityManager` invocations only exist in the raw source. All other detectors work purely off the parsed model.

Implementation: [`projectmind_core::patterns`](../crates/core/src/patterns.rs). Fixture tests (clean repo *holds* + drifting repo *violates*) driven through the full Java + Spring pipeline live in [`crates/mcp-server/tests/patterns.rs`](../crates/mcp-server/tests/patterns.rs).

## Confidence & noise suppression

Every violation carries a per-hit `confidence` (0.0–1.0) reflecting how clearly it matches the rule:

- a field typed `EntityManager` (0.9) vs. a body-scan `entityManager.` call (0.7),
- a `new UserService()` of a *known* bean (0.85) vs. a `new OrderService()` matched only by naming convention (0.65),
- a static `Map<…>` cache (0.9) vs. a plain static scalar (0.75).

Violations below **0.6** (`patterns::CONFIDENCE_FLOOR`) are hidden from the heatmap to keep the noise down. The `PatternResult` also carries a detector-level `confidence` for how reliably the detector as a whole separates drift from clean code.

## Configuration — `.projectmind/patterns.toml`

A missing file means **every detector runs with defaults**. Present keys overlay the defaults; unknown keys are tolerated.

```toml
[layers]
# Outer → inner. A layer may depend on those to its right, never to its left.
order = ["web", "service", "repository", "domain"]

[layers.package_matchers]
# `|`-separated `*.suffix` globs, matched against a class FQN. Overlaying only
# one layer keeps the sensible defaults for the others.
web = "*.web|*.controller|*.rest|*.api"
service = "*.service"
repository = "*.repository|*.dao|*.entity"
domain = "*.domain|*.model"

[patterns]
# Detectors to switch off, by label (PascalCase) or snake_case id.
disable = ["NoStaticState"]
```

Defaults are exactly the values shown above. `PatternConfig::load(repo_root)` reads the file; a malformed file falls back to defaults rather than blanking the whole lens.

## Surfaces

- **MCP tool `pattern_check`** — `{ pattern: "repository" | "layered" | "di_only" | "tx_on_service" | "no_static_state", module?: "<id>" }` → `{ holds:[{module,count}], violations:[{file,line,message,severity,confidence}], confidence }`. PascalCase labels (`Repository`, `DI`, `Transactional`, …) are accepted too.
- **Tauri command `pattern_check(pattern?, module?)`** and **browser-host `GET /api/pattern_check?pattern=&module=`** — both return the whole heatmap (all enabled detectors) when `pattern` is omitted, or a single detector when it's given. This is what the **Patterns** tab consumes.

## Tradeoffs / roadmap

- Detectors are heuristics; false positives will become suppressible via an inline annotation (`// @projectmind:allow Repository`, planned 2.3.1).
- Layered detection is intra-module in v1; proper Maven multi-module layering needs an inter-module relation pass.
