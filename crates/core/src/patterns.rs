// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pattern Lens — architectural-pattern compliance detectors.
//!
//! Each detector returns a [`PatternResult`] describing how often the rule
//! holds versus how often it drifts. Most detectors read from the already-
//! parsed [`Repository`] (classes, annotations, fields), so they're cheap
//! to run repeatedly. Two detectors ([`Pattern::Repository`] and
//! [`Pattern::DiOnly`]) additionally scan the class's source text within its
//! line range, because the parse model does not capture method bodies —
//! constructor calls (`new UserService()`) and direct `EntityManager`
//! invocations only exist in the raw source.
//!
//! v1 detectors (#159):
//! - [`Pattern::Repository`] — only `@Repository` classes may touch
//!   `EntityManager` / `JdbcTemplate` / `JpaRepository` directly. A
//!   `@Service` that reaches for `EntityManager` is drift.
//! - [`Pattern::Layered`] — `*.web` → `*.service` → `*.repository`, never
//!   backwards. A web/controller class that references a repository or
//!   entity directly is drift. v1 flags intra-module only (see the module
//!   docs / PR): cross-module layering needs an inter-module relation pass.
//! - [`Pattern::DiOnly`] — no manual `new XxxService()` inside a
//!   `@Component`-typed class; wire collaborators via DI instead.
//! - [`Pattern::TxOnService`] — `@Transactional` must live on `@Service`
//!   (or a repository custom-query), not on `@Controller` /
//!   `@RestController`.
//! - [`Pattern::NoStaticState`] — Spring components must not own non-final
//!   `static` fields (mutable shared state breaks lifecycle assumptions).
//!
//! Layer rules and detector on/off flags are configurable per repo via
//! `.projectmind/patterns.toml` (see [`PatternConfig`]). A missing file
//! means every detector runs with built-in defaults.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use projectmind_plugin_api::Class;
use serde::{Deserialize, Serialize};

use crate::repository::Repository;

/// Patterns understood by [`check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    /// Only `@Repository` classes touch `EntityManager` / `JdbcTemplate` /
    /// `JpaRepository` directly.
    Repository,
    /// Controllers / web classes must not reference repositories / entities directly.
    Layered,
    /// No manual `new XxxService()` inside `@Component`-typed classes.
    DiOnly,
    /// `@Transactional` only on `@Service` (not on controllers).
    TxOnService,
    /// No non-final `static` fields on Spring `@Component`-typed classes.
    NoStaticState,
}

impl Pattern {
    /// Every pattern, in the order shown in the compliance heatmap.
    pub const ALL: [Self; 5] = [
        Self::Repository,
        Self::Layered,
        Self::DiOnly,
        Self::TxOnService,
        Self::NoStaticState,
    ];

    /// Parse a pattern name. Accepts snake_case, the compact form, and the
    /// PascalCase label used in the issue / MCP tool docs.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "repository" | "Repository" => Some(Self::Repository),
            "layered" | "Layered" => Some(Self::Layered),
            "di" | "di_only" | "dionly" | "DI" | "DiOnly" => Some(Self::DiOnly),
            "tx_on_service" | "txonservice" | "transactional" | "Transactional" | "TxOnService" => {
                Some(Self::TxOnService)
            }
            "no_static_state" | "nostaticstate" | "NoStaticState" => Some(Self::NoStaticState),
            _ => None,
        }
    }

    /// Stable snake-case name for serialisation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Repository => "repository",
            Self::Layered => "layered",
            Self::DiOnly => "di_only",
            Self::TxOnService => "tx_on_service",
            Self::NoStaticState => "no_static_state",
        }
    }

    /// PascalCase label (matches the config `patterns.disable` list and the
    /// issue's heatmap rows).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Repository => "Repository",
            Self::Layered => "Layered",
            Self::DiOnly => "DI",
            Self::TxOnService => "Transactional",
            Self::NoStaticState => "NoStaticState",
        }
    }
}

/// One module's compliance count: classes that fully satisfy the rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleHold {
    /// Module id.
    pub module: String,
    /// Classes inside this module that satisfy the rule.
    pub count: u32,
}

/// One concrete violation of a pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    /// Module id where the violation lives.
    pub module: String,
    /// Repo-relative file path (best effort — falls back to module-relative).
    pub file: PathBuf,
    /// 1-based line. `0` when the offending element has no line attached.
    pub line: u32,
    /// FQN of the offending class.
    pub fqn: String,
    /// Human-readable summary of the violation.
    pub message: String,
    /// Severity, 1=info, 2=warn, 3=critical.
    pub severity: u8,
    /// How clearly this specific hit matches the rule (0.0..=1.0). Violations
    /// below [`CONFIDENCE_FLOOR`] are noise-suppressed from the heatmap.
    pub confidence: f64,
}

/// Aggregate result returned by [`check`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternResult {
    /// Pattern that was checked.
    pub pattern: Pattern,
    /// Per-module count of classes that satisfy the rule.
    pub holds: Vec<ModuleHold>,
    /// All violations found (including low-confidence ones — callers that
    /// render the heatmap should filter on [`CONFIDENCE_FLOOR`]).
    pub violations: Vec<Violation>,
    /// Detector-level confidence: how reliably this detector distinguishes
    /// real drift from clean code, given the available signals.
    pub confidence: f64,
}

/// Violations with a confidence below this floor are hidden from the
/// compliance heatmap to keep the noise down (issue #159 tradeoff).
pub const CONFIDENCE_FLOOR: f64 = 0.6;

impl PatternResult {
    /// Violations at or above [`CONFIDENCE_FLOOR`] — what the heatmap shows.
    #[must_use]
    pub fn visible_violations(&self) -> Vec<&Violation> {
        self.violations
            .iter()
            .filter(|v| v.confidence >= CONFIDENCE_FLOOR)
            .collect()
    }
}

/// Optional scope passed to [`check`].
#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// Filter to a single module id when set.
    pub module: Option<String>,
}

/// Per-repo pattern configuration, parsed from `.projectmind/patterns.toml`.
///
/// A missing file yields [`PatternConfig::default`] — every detector on,
/// with the default layer order. Unknown keys are tolerated so a newer
/// client that adds sections doesn't break an older reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternConfig {
    /// Ordered layer names, outer → inner (`web` may depend on `service`,
    /// `service` on `repository`, never the reverse).
    pub layer_order: Vec<String>,
    /// Glob-ish package matchers per layer. A matcher is a `|`-separated
    /// list of `*.suffix` globs matched against a class FQN.
    pub package_matchers: BTreeMap<String, String>,
    /// Detectors switched off by label (`"NoStaticState"`, `"Repository"`, …).
    pub disabled: Vec<String>,
}

impl Default for PatternConfig {
    fn default() -> Self {
        let mut package_matchers = BTreeMap::new();
        package_matchers.insert(
            "web".to_string(),
            "*.web|*.controller|*.rest|*.api".to_string(),
        );
        package_matchers.insert("service".to_string(), "*.service".to_string());
        package_matchers.insert(
            "repository".to_string(),
            "*.repository|*.dao|*.entity".to_string(),
        );
        package_matchers.insert("domain".to_string(), "*.domain|*.model".to_string());
        Self {
            layer_order: vec![
                "web".to_string(),
                "service".to_string(),
                "repository".to_string(),
                "domain".to_string(),
            ],
            package_matchers,
            disabled: Vec::new(),
        }
    }
}

/// Raw TOML shape (mirrors the `[layers]` / `[patterns]` sections).
#[derive(Debug, Default, Deserialize)]
struct PatternConfigFile {
    #[serde(default)]
    layers: LayersSection,
    #[serde(default)]
    patterns: PatternsSection,
}

#[derive(Debug, Default, Deserialize)]
struct LayersSection {
    #[serde(default)]
    order: Option<Vec<String>>,
    #[serde(default)]
    package_matchers: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Default, Deserialize)]
struct PatternsSection {
    #[serde(default)]
    disable: Vec<String>,
}

impl PatternConfig {
    /// Config file location inside a repo.
    #[must_use]
    pub fn config_path(repo_root: &Path) -> PathBuf {
        repo_root.join(".projectmind").join("patterns.toml")
    }

    /// Load the effective config for `repo_root`. A missing file is not an
    /// error — [`PatternConfig::default`] applies. A malformed file falls
    /// back to defaults too (with the parse error surfaced to the caller),
    /// so a typo never blanks the whole lens.
    #[must_use]
    pub fn load(repo_root: &Path) -> Self {
        match std::fs::read_to_string(Self::config_path(repo_root)) {
            Ok(text) => Self::parse(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Parse a `patterns.toml` document, overlaying defaults for absent keys.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: PatternConfigFile = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut cfg = Self::default();
        if let Some(order) = file.layers.order {
            if !order.is_empty() {
                cfg.layer_order = order;
            }
        }
        if let Some(matchers) = file.layers.package_matchers {
            // Overlay rather than replace: an author who tweaks only `web`
            // keeps the sensible defaults for the other layers.
            for (k, v) in matchers {
                cfg.package_matchers.insert(k, v);
            }
        }
        cfg.disabled = file.patterns.disable;
        Ok(cfg)
    }

    /// Whether a detector is enabled (not listed in `patterns.disable`).
    #[must_use]
    pub fn enabled(&self, pattern: Pattern) -> bool {
        !self
            .disabled
            .iter()
            .any(|d| d.eq_ignore_ascii_case(pattern.label()) || d == pattern.as_str())
    }

    /// Return the layer name whose matcher accepts `fqn`, if any.
    fn layer_of(&self, fqn: &str) -> Option<&str> {
        let lower = fqn.to_ascii_lowercase();
        self.layer_order
            .iter()
            .find(|layer| {
                self.package_matchers
                    .get(*layer)
                    .is_some_and(|glob| matches_package(&lower, glob))
            })
            .map(String::as_str)
    }
}

/// Match a lower-cased FQN against a `|`-separated list of `*.suffix` globs.
/// `*.web` matches any FQN containing a `.web.` package segment or ending in
/// `.web`.
fn matches_package(lower_fqn: &str, glob: &str) -> bool {
    glob.split('|').any(|part| {
        let seg = part.trim().trim_start_matches("*.").trim_start_matches('.');
        if seg.is_empty() {
            return false;
        }
        lower_fqn.contains(&format!(".{seg}.")) || lower_fqn.ends_with(&format!(".{seg}"))
    })
}

/// Run a pattern check against `repo` with default configuration.
#[must_use]
pub fn check(repo: &Repository, pattern: Pattern, scope: &Scope) -> PatternResult {
    check_with_config(repo, pattern, scope, &PatternConfig::default())
}

/// Run a pattern check against `repo` with an explicit [`PatternConfig`].
#[must_use]
pub fn check_with_config(
    repo: &Repository,
    pattern: Pattern,
    scope: &Scope,
    config: &PatternConfig,
) -> PatternResult {
    match pattern {
        Pattern::Repository => check_repository(repo, scope),
        Pattern::Layered => check_layered(repo, scope, config),
        Pattern::DiOnly => check_di_only(repo, scope),
        Pattern::TxOnService => check_tx_on_service(repo, scope),
        Pattern::NoStaticState => check_no_static_state(repo, scope),
    }
}

/// Run every enabled detector, honouring the repo's [`PatternConfig`]. This is
/// what the heatmap consumes: one [`PatternResult`] per active pattern.
#[must_use]
pub fn check_all(repo: &Repository, scope: &Scope, config: &PatternConfig) -> Vec<PatternResult> {
    Pattern::ALL
        .iter()
        .filter(|p| config.enabled(**p))
        .map(|p| check_with_config(repo, *p, scope, config))
        .collect()
}

fn each_class<'a>(
    repo: &'a Repository,
    scope: &'a Scope,
) -> impl Iterator<Item = (&'a str, &'a Path, &'a Class)> + 'a {
    repo.modules
        .values()
        .filter(move |m| scope.module.as_deref().map_or(true, |f| f == m.id))
        .flat_map(|m| {
            m.classes
                .values()
                .map(move |c| (m.id.as_str(), m.root.as_path(), c))
        })
}

fn rel_file(_module_root: &Path, class_file: &Path) -> PathBuf {
    class_file.to_path_buf()
}

/// Read the source lines that make up `class` (1-based, inclusive). Returns
/// `None` when the file can't be read — the caller then falls back to
/// signals available on the parsed model.
fn class_source(module_root: &Path, class: &Class) -> Option<String> {
    let abs = module_root.join(&class.file);
    let text = std::fs::read_to_string(abs).ok()?;
    let start = class.line_start.max(1) as usize - 1;
    let end = class.line_end.max(class.line_start) as usize;
    let slice: Vec<&str> = text.lines().skip(start).take(end - start).collect();
    Some(slice.join("\n"))
}

// --- Detector: Repository -------------------------------------------------

/// The persistence primitives only a `@Repository` should reach for.
const PERSISTENCE_APIS: &[&str] = &[
    "EntityManager",
    "JdbcTemplate",
    "NamedParameterJdbcTemplate",
];

fn check_repository(repo: &Repository, scope: &Scope) -> PatternResult {
    let mut holds: BTreeMap<String, u32> = BTreeMap::new();
    let mut violations: Vec<Violation> = Vec::new();

    for (module_id, module_root, class) in each_class(repo, scope) {
        // Only Spring beans that are *not* repositories are in scope: a
        // `@Service` / `@Component` that pokes at persistence primitives is
        // the drift we're after. Repositories are allowed to.
        if !is_spring_component(class) || is_spring_repository(class) {
            continue;
        }

        let mut hits: Vec<(String, u32, f64)> = Vec::new();

        // Strongest signal: a field typed as a persistence primitive, or one
        // annotated @PersistenceContext — no body scan needed, high confidence.
        for f in &class.fields {
            if let Some(api) = persistence_api(&f.type_text) {
                hits.push((format!("field `{}: {api}`", f.name), f.line, 0.9));
            } else if f.annotations.iter().any(|a| a.is("PersistenceContext")) {
                hits.push((
                    format!("field `{}` is @PersistenceContext-injected", f.name),
                    f.line,
                    0.9,
                ));
            }
        }

        // Body signal: `entityManager.` / `jdbcTemplate.` calls in the source.
        // Lower confidence — a comment or string could mention the type.
        if hits.is_empty() {
            if let Some(src) = class_source(module_root, class) {
                if let Some((api, line)) = find_persistence_call(&src, class.line_start) {
                    hits.push((format!("calls `{api}` directly"), line, 0.7));
                }
            }
        }

        if hits.is_empty() {
            *holds.entry(module_id.to_string()).or_default() += 1;
        } else {
            for (what, line, confidence) in hits {
                violations.push(Violation {
                    module: module_id.to_string(),
                    file: rel_file(module_root, &class.file),
                    line,
                    fqn: class.fqn.clone(),
                    message: format!(
                        "`{}` ({}) reaches into persistence — {what}; keep `EntityManager` / `JdbcTemplate` behind an @Repository",
                        class.name,
                        stereotype_label(class),
                    ),
                    severity: 3,
                    confidence,
                });
            }
        }
    }

    PatternResult {
        pattern: Pattern::Repository,
        holds: holds_to_vec(holds),
        violations,
        confidence: 0.85,
    }
}

/// If `type_text` denotes a persistence primitive, return its canonical name.
fn persistence_api(type_text: &str) -> Option<&'static str> {
    let simple = type_text
        .rsplit(['.', ' ', '<'])
        .find(|s| !s.is_empty())
        .unwrap_or(type_text);
    PERSISTENCE_APIS
        .iter()
        .find(|api| api.eq_ignore_ascii_case(simple))
        .copied()
}

/// Scan a class body for a direct `entityManager.` / `jdbcTemplate.` call.
/// Returns the API name and the absolute (1-based) source line.
fn find_persistence_call(src: &str, class_line_start: u32) -> Option<(&'static str, u32)> {
    const CALLS: &[(&str, &str)] = &[
        ("entityManager.", "EntityManager"),
        ("getEntityManager(", "EntityManager"),
        ("jdbcTemplate.", "JdbcTemplate"),
    ];
    for (offset, line) in src.lines().enumerate() {
        let code = strip_line_comment(line);
        for (needle, api) in CALLS {
            if code.contains(needle) {
                return Some((api, class_line_start + offset as u32));
            }
        }
    }
    None
}

// --- Detector: DI-only ----------------------------------------------------

fn check_di_only(repo: &Repository, scope: &Scope) -> PatternResult {
    // Simple names of every Spring bean in the repo: `new`-ing one of these
    // by hand is the drift (`new UserService()` instead of injecting it).
    let bean_simple_names: Vec<String> = repo
        .modules
        .values()
        .flat_map(|m| m.classes.values())
        .filter(|c| is_spring_component(c))
        .map(|c| c.name.clone())
        .collect();

    let mut holds: BTreeMap<String, u32> = BTreeMap::new();
    let mut violations: Vec<Violation> = Vec::new();

    for (module_id, module_root, class) in each_class(repo, scope) {
        if !is_spring_component(class) {
            continue;
        }

        let mut clean = true;
        if let Some(src) = class_source(module_root, class) {
            for (target, line, confidence) in
                find_bean_instantiations(&src, class.line_start, &bean_simple_names, &class.name)
            {
                clean = false;
                violations.push(Violation {
                    module: module_id.to_string(),
                    file: rel_file(module_root, &class.file),
                    line,
                    fqn: class.fqn.clone(),
                    message: format!(
                        "`{}` instantiates `new {target}(…)` by hand — inject the bean instead of new-ing it",
                        class.name
                    ),
                    severity: 2,
                    confidence,
                });
            }
        }
        if clean {
            *holds.entry(module_id.to_string()).or_default() += 1;
        }
    }

    PatternResult {
        pattern: Pattern::DiOnly,
        holds: holds_to_vec(holds),
        violations,
        confidence: 0.8,
    }
}

/// Find `new XxxService(` / `new XxxRepository(` / `new <bean>(` expressions.
/// Returns `(target, line, confidence)` for each hit.
fn find_bean_instantiations(
    src: &str,
    class_line_start: u32,
    bean_simple_names: &[String],
    self_name: &str,
) -> Vec<(String, u32, f64)> {
    let mut out = Vec::new();
    for (offset, line) in src.lines().enumerate() {
        let code = strip_line_comment(line);
        let mut rest = code;
        while let Some(pos) = rest.find("new ") {
            let after = &rest[pos + 4..];
            let type_name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            // Advance past this match before deciding, so multiple `new`s on
            // one line are all seen.
            rest = &after[type_name.len()..];
            if type_name.is_empty() || type_name == self_name {
                continue;
            }
            let is_known_bean = bean_simple_names.iter().any(|n| n == &type_name);
            // A `new` of a known bean is a strong hit; a `new XxxService(`
            // whose type isn't a known bean (e.g. cross-module) is a weaker
            // by-convention hit.
            let looks_like_service = type_name.ends_with("Service")
                || type_name.ends_with("Repository")
                || type_name.ends_with("Controller")
                || type_name.ends_with("Component");
            let followed_by_call = rest.trim_start().starts_with('(');
            if !followed_by_call {
                continue;
            }
            if is_known_bean {
                out.push((type_name, class_line_start + offset as u32, 0.85));
            } else if looks_like_service {
                out.push((type_name, class_line_start + offset as u32, 0.65));
            }
        }
    }
    out
}

// --- Detector: @Transactional boundary ------------------------------------

fn check_tx_on_service(repo: &Repository, scope: &Scope) -> PatternResult {
    let mut holds: BTreeMap<String, u32> = BTreeMap::new();
    let mut violations: Vec<Violation> = Vec::new();

    for (module_id, module_root, class) in each_class(repo, scope) {
        let on_class = class.annotations.iter().any(|a| a.is("Transactional"));
        let methods_with_tx: Vec<&projectmind_plugin_api::Method> = class
            .methods
            .iter()
            .filter(|m| m.annotations.iter().any(|a| a.is("Transactional")))
            .collect();

        let has_tx_anywhere = on_class || !methods_with_tx.is_empty();
        if !has_tx_anywhere {
            continue;
        }

        if is_spring_service_or_repo(class) {
            *holds.entry(module_id.to_string()).or_default() += 1;
            continue;
        }

        if is_spring_controller(class) {
            let line = if on_class {
                class.line_start
            } else {
                methods_with_tx
                    .first()
                    .map_or(class.line_start, |m| m.line_start)
            };
            violations.push(Violation {
                module: module_id.to_string(),
                file: rel_file(module_root, &class.file),
                line,
                fqn: class.fqn.clone(),
                message: format!(
                    "Controller `{}` carries @Transactional — move boundary to @Service",
                    class.name
                ),
                severity: 3,
                confidence: 0.9,
            });
        }
    }

    PatternResult {
        pattern: Pattern::TxOnService,
        holds: holds_to_vec(holds),
        violations,
        confidence: 0.85,
    }
}

// --- Detector: Layered ----------------------------------------------------

fn check_layered(repo: &Repository, scope: &Scope, config: &PatternConfig) -> PatternResult {
    let mut holds: BTreeMap<String, u32> = BTreeMap::new();
    let mut violations: Vec<Violation> = Vec::new();

    for (module_id, module_root, class) in each_class(repo, scope) {
        let Some(layer) = config.layer_of(&class.fqn) else {
            continue;
        };
        // Only the outermost `web` layer is inspected in v1 (intra-module).
        if layer != "web" {
            continue;
        }
        let mut bad_refs: Vec<String> = Vec::new();
        for f in &class.fields {
            if is_persistence_ref(&f.type_text) {
                bad_refs.push(format!("field `{}: {}`", f.name, f.type_text));
            }
        }
        for s in &class.super_types {
            if is_persistence_ref(&s.name) {
                bad_refs.push(format!("super-type `{}`", s.name));
            }
        }

        if bad_refs.is_empty() {
            *holds.entry(module_id.to_string()).or_default() += 1;
        } else {
            for r in bad_refs {
                violations.push(Violation {
                    module: module_id.to_string(),
                    file: rel_file(module_root, &class.file),
                    line: class.line_start,
                    fqn: class.fqn.clone(),
                    message: format!(
                        "Web class `{}` reaches into persistence layer ({r}) — go through a service",
                        class.name
                    ),
                    severity: 2,
                    confidence: 0.8,
                });
            }
        }
    }

    PatternResult {
        pattern: Pattern::Layered,
        holds: holds_to_vec(holds),
        violations,
        confidence: 0.8,
    }
}

// --- Detector: No static state --------------------------------------------

fn check_no_static_state(repo: &Repository, scope: &Scope) -> PatternResult {
    let mut holds: BTreeMap<String, u32> = BTreeMap::new();
    let mut violations: Vec<Violation> = Vec::new();

    for (module_id, module_root, class) in each_class(repo, scope) {
        if !is_spring_component(class) {
            continue;
        }
        let bad_fields: Vec<&projectmind_plugin_api::Field> = class
            .fields
            .iter()
            .filter(|f| f.is_static && !is_final_field(f))
            .collect();

        if bad_fields.is_empty() {
            *holds.entry(module_id.to_string()).or_default() += 1;
        } else {
            for f in bad_fields {
                // A mutable collection is the textbook shared-cache smell —
                // rank it a touch higher than a plain scalar.
                let confidence = if is_collection_type(&f.type_text) {
                    0.9
                } else {
                    0.75
                };
                violations.push(Violation {
                    module: module_id.to_string(),
                    file: rel_file(module_root, &class.file),
                    line: f.line,
                    fqn: class.fqn.clone(),
                    message: format!(
                        "Spring component `{}` owns mutable static field `{}` — replace with bean state or a singleton",
                        class.name, f.name
                    ),
                    severity: 2,
                    confidence,
                });
            }
        }
    }

    PatternResult {
        pattern: Pattern::NoStaticState,
        holds: holds_to_vec(holds),
        violations,
        confidence: 0.85,
    }
}

// --- Shared helpers -------------------------------------------------------

fn holds_to_vec(holds: BTreeMap<String, u32>) -> Vec<ModuleHold> {
    holds
        .into_iter()
        .map(|(module, count)| ModuleHold { module, count })
        .collect()
}

/// Drop a trailing `//` line comment so scans ignore commented-out code.
fn strip_line_comment(line: &str) -> &str {
    line.find("//").map_or(line, |i| &line[..i])
}

fn is_collection_type(type_text: &str) -> bool {
    let lower = type_text.to_ascii_lowercase();
    [
        "map<",
        "list<",
        "set<",
        "collection<",
        "map ",
        "list ",
        "set ",
    ]
    .iter()
    .any(|t| lower.contains(t))
        || lower.ends_with("map")
        || lower.ends_with("list")
        || lower.ends_with("set")
}

fn stereotype_label(class: &Class) -> &'static str {
    if is_spring_service_or_repo(class) {
        "@Service"
    } else if is_spring_controller(class) {
        "@Controller"
    } else {
        "@Component"
    }
}

fn is_spring_component(class: &Class) -> bool {
    const TAGS: &[&str] = &[
        "Component",
        "Service",
        "Repository",
        "Controller",
        "RestController",
        "Configuration",
    ];
    class
        .annotations
        .iter()
        .any(|a| TAGS.iter().any(|t| a.is(t)))
        || class.stereotypes.iter().any(|s| {
            matches!(
                s.as_str(),
                "component"
                    | "service"
                    | "repository"
                    | "controller"
                    | "rest-controller"
                    | "configuration"
            )
        })
}

fn is_spring_service_or_repo(class: &Class) -> bool {
    class
        .annotations
        .iter()
        .any(|a| a.is("Service") || a.is("Repository"))
        || class
            .stereotypes
            .iter()
            .any(|s| s == "service" || s == "repository")
}

fn is_spring_repository(class: &Class) -> bool {
    class.annotations.iter().any(|a| a.is("Repository"))
        || class.stereotypes.iter().any(|s| s == "repository")
}

fn is_spring_controller(class: &Class) -> bool {
    class
        .annotations
        .iter()
        .any(|a| a.is("Controller") || a.is("RestController"))
        || class
            .stereotypes
            .iter()
            .any(|s| s == "controller" || s == "rest-controller")
}

fn is_final_field(field: &projectmind_plugin_api::Field) -> bool {
    // Primary signal: the parser extracted the `final` modifier. The
    // type_text checks stay as a fallback for parsers that fold modifiers
    // into the type text; `final` itself is a keyword, never an annotation.
    field.is_final
        || field
            .annotations
            .iter()
            .any(|a| a.is("Value") || a.is("ConfigurationProperty"))
        || field.type_text.starts_with("final ")
        || field.type_text.contains(" final ")
}

fn is_persistence_ref(type_text: &str) -> bool {
    let lower = type_text.to_ascii_lowercase();
    lower.contains(".repository.")
        || lower.contains(".entity.")
        || lower.contains(".dao.")
        || lower.ends_with("repository")
        || lower.ends_with("entity")
        || lower.ends_with("entitymanager")
        || lower == "entitymanager"
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectmind_plugin_api::{Annotation, Class, ClassKind, Field, Method, Module, TypeRef};

    fn mk_repo(modules: Vec<Module>) -> Repository {
        let mut r = Repository::new(PathBuf::from("/tmp/patterns"));
        for m in modules {
            r.insert_module(m);
        }
        r
    }

    fn make_module(id: &str, classes: Vec<Class>) -> Module {
        let mut m = Module {
            id: id.into(),
            name: id.into(),
            root: PathBuf::from(format!("/tmp/{id}")),
            ..Default::default()
        };
        for c in classes {
            m.classes.insert(c.fqn.clone(), c);
        }
        m
    }

    fn annot(name: &str) -> Annotation {
        Annotation {
            name: name.into(),
            fqn: None,
            raw_args: None,
        }
    }

    fn cls(fqn: &str) -> Class {
        Class {
            fqn: fqn.into(),
            name: fqn.rsplit('.').next().unwrap_or(fqn).into(),
            file: PathBuf::from(format!("{}.java", fqn.rsplit('.').next().unwrap_or("Cls"))),
            kind: ClassKind::Class,
            line_start: 10,
            line_end: 50,
            ..Default::default()
        }
    }

    // ---- Repository detector (field-based; body-based is fixture-tested) ----

    #[test]
    fn repository_flags_service_with_entitymanager_field() {
        let mut c = cls("a.service.UserService");
        c.annotations.push(annot("Service"));
        c.fields.push(Field {
            name: "em".into(),
            type_text: "EntityManager".into(),
            line: 15,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::Repository, &Scope::default());
        assert_eq!(res.violations.len(), 1);
        assert_eq!(res.violations[0].line, 15);
        assert!(res.violations[0].confidence >= CONFIDENCE_FLOOR);
    }

    #[test]
    fn repository_flags_persistence_context_field() {
        let mut c = cls("a.service.OrderService");
        c.annotations.push(annot("Service"));
        c.fields.push(Field {
            name: "em".into(),
            type_text: "Object".into(),
            line: 20,
            annotations: vec![annot("PersistenceContext")],
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::Repository, &Scope::default());
        assert_eq!(res.violations.len(), 1);
    }

    #[test]
    fn repository_allows_repository_with_entitymanager() {
        let mut c = cls("a.repository.UserRepo");
        c.annotations.push(annot("Repository"));
        c.fields.push(Field {
            name: "em".into(),
            type_text: "EntityManager".into(),
            line: 12,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::Repository, &Scope::default());
        assert!(res.violations.is_empty());
        // A repository is skipped entirely (not in scope), so it isn't a "hold".
        assert!(res.holds.is_empty());
    }

    #[test]
    fn repository_holds_for_service_using_jparepository() {
        let mut c = cls("a.service.CleanService");
        c.annotations.push(annot("Service"));
        c.fields.push(Field {
            name: "repo".into(),
            type_text: "UserRepository".into(),
            line: 12,
            annotations: vec![annot("Autowired")],
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::Repository, &Scope::default());
        assert!(res.violations.is_empty());
        assert_eq!(res.holds[0].count, 1);
    }

    // ---- DI-only detector helper (line scanner) ----

    #[test]
    fn di_scanner_flags_new_of_known_bean() {
        let beans = vec!["UserService".to_string()];
        let hits = find_bean_instantiations(
            "        this.svc = new UserService();",
            10,
            &beans,
            "UserController",
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "UserService");
        assert_eq!(hits[0].1, 10);
        assert!(hits[0].2 >= 0.6);
    }

    #[test]
    fn di_scanner_flags_service_by_convention() {
        let beans: Vec<String> = Vec::new();
        let hits = find_bean_instantiations("var s = new OrderService(a, b);", 5, &beans, "X");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].2 < CONFIDENCE_FLOOR + 0.1); // weaker by-convention hit
    }

    #[test]
    fn di_scanner_ignores_new_of_plain_type() {
        let beans: Vec<String> = Vec::new();
        let hits = find_bean_instantiations("var list = new ArrayList<>();", 5, &beans, "X");
        assert!(hits.is_empty());
    }

    #[test]
    fn di_scanner_ignores_commented_out_new() {
        let beans = vec!["UserService".to_string()];
        let hits = find_bean_instantiations("// this.svc = new UserService();", 5, &beans, "X");
        assert!(hits.is_empty());
    }

    #[test]
    fn di_scanner_ignores_self_type() {
        let beans = vec!["Builder".to_string()];
        let hits = find_bean_instantiations("return new Builder();", 5, &beans, "Builder");
        assert!(hits.is_empty());
    }

    // ---- NoStaticState ----

    #[test]
    fn no_static_state_flags_mutable_static_in_component() {
        let mut c = cls("a.b.Cache");
        c.annotations.push(annot("Service"));
        c.fields.push(Field {
            name: "MAP".into(),
            type_text: "Map<String,String>".into(),
            line: 12,
            is_static: true,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::NoStaticState, &Scope::default());
        assert_eq!(res.violations.len(), 1);
        assert_eq!(res.violations[0].line, 12);
        assert!(res.violations[0].message.contains("MAP"));
        // Collection cache smell → higher confidence.
        assert!(res.violations[0].confidence >= 0.9);
    }

    #[test]
    fn no_static_state_ignores_final_static() {
        let mut c = cls("a.b.K");
        c.annotations.push(annot("Component"));
        c.fields.push(Field {
            name: "MAX".into(),
            type_text: "final int".into(),
            line: 11,
            is_static: true,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::NoStaticState, &Scope::default());
        assert!(res.violations.is_empty());
        assert_eq!(res.holds.len(), 1);
        assert_eq!(res.holds[0].count, 1);
    }

    #[test]
    fn no_static_state_ignores_static_final_via_is_final_flag() {
        let mut c = cls("a.b.Svc");
        c.annotations.push(annot("Service"));
        c.fields.push(Field {
            name: "LOG".into(),
            type_text: "Logger".into(),
            line: 7,
            is_static: true,
            is_final: true,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::NoStaticState, &Scope::default());
        assert!(res.violations.is_empty());
        assert_eq!(res.holds.len(), 1);
        assert_eq!(res.holds[0].count, 1);
    }

    #[test]
    fn no_static_state_ignores_non_components() {
        let mut c = cls("a.b.Util");
        c.fields.push(Field {
            name: "X".into(),
            type_text: "int".into(),
            line: 1,
            is_static: true,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::NoStaticState, &Scope::default());
        assert!(res.violations.is_empty());
    }

    // ---- TxOnService ----

    #[test]
    fn tx_on_service_flags_controller_with_tx() {
        let mut c = cls("a.web.UserCtrl");
        c.annotations.push(annot("RestController"));
        let m = Method {
            name: "update".into(),
            line_start: 25,
            annotations: vec![annot("Transactional")],
            ..Default::default()
        };
        c.methods.push(m);
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::TxOnService, &Scope::default());
        assert_eq!(res.violations.len(), 1);
        assert_eq!(res.violations[0].line, 25);
    }

    #[test]
    fn tx_on_service_accepts_service_with_tx() {
        let mut c = cls("a.svc.UserSvc");
        c.annotations.push(annot("Service"));
        c.annotations.push(annot("Transactional"));
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::TxOnService, &Scope::default());
        assert!(res.violations.is_empty());
        assert_eq!(res.holds[0].count, 1);
    }

    // ---- Layered ----

    #[test]
    fn layered_flags_controller_with_repository_field() {
        let mut c = cls("a.controller.UserCtrl");
        c.annotations.push(annot("RestController"));
        c.fields.push(Field {
            name: "repo".into(),
            type_text: "a.repository.UserRepository".into(),
            line: 14,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::Layered, &Scope::default());
        assert_eq!(res.violations.len(), 1);
        assert!(res.violations[0].message.contains("persistence"));
    }

    #[test]
    fn layered_flags_controller_extending_entity() {
        let mut c = cls("a.web.AdminCtrl");
        c.annotations.push(annot("Controller"));
        c.super_types.push(TypeRef {
            name: "a.entity.User".into(),
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::Layered, &Scope::default());
        assert_eq!(res.violations.len(), 1);
    }

    #[test]
    fn layered_holds_for_clean_controller() {
        let mut c = cls("a.controller.UserCtrl");
        c.annotations.push(annot("Controller"));
        c.fields.push(Field {
            name: "svc".into(),
            type_text: "a.service.UserService".into(),
            line: 14,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::Layered, &Scope::default());
        assert!(res.violations.is_empty());
        assert_eq!(res.holds[0].count, 1);
    }

    #[test]
    fn scope_filter_limits_to_module() {
        let mut c1 = cls("x.web.Ctl");
        c1.annotations.push(annot("Controller"));
        c1.fields.push(Field {
            name: "r".into(),
            type_text: "x.repository.Repo".into(),
            line: 5,
            ..Default::default()
        });
        let mut c2 = cls("y.web.Ctl");
        c2.annotations.push(annot("Controller"));
        c2.fields.push(Field {
            name: "r".into(),
            type_text: "y.repository.Repo".into(),
            line: 5,
            ..Default::default()
        });
        let repo = mk_repo(vec![
            make_module("alpha", vec![c1]),
            make_module("beta", vec![c2]),
        ]);
        let res = check(
            &repo,
            Pattern::Layered,
            &Scope {
                module: Some("alpha".into()),
            },
        );
        assert_eq!(res.violations.len(), 1);
        assert_eq!(res.violations[0].module, "alpha");
    }

    // ---- Parsing / config ----

    #[test]
    fn pattern_parses_aliases() {
        assert_eq!(Pattern::parse("layered"), Some(Pattern::Layered));
        assert_eq!(Pattern::parse("Layered"), Some(Pattern::Layered));
        assert_eq!(
            Pattern::parse("no_static_state"),
            Some(Pattern::NoStaticState)
        );
        assert_eq!(Pattern::parse("tx_on_service"), Some(Pattern::TxOnService));
        assert_eq!(Pattern::parse("Transactional"), Some(Pattern::TxOnService));
        assert_eq!(Pattern::parse("Repository"), Some(Pattern::Repository));
        assert_eq!(Pattern::parse("DI"), Some(Pattern::DiOnly));
        assert_eq!(Pattern::parse("di_only"), Some(Pattern::DiOnly));
        assert!(Pattern::parse("unknown").is_none());
    }

    #[test]
    fn config_defaults_enable_everything() {
        let cfg = PatternConfig::default();
        for p in Pattern::ALL {
            assert!(cfg.enabled(p), "{p:?} should be on by default");
        }
    }

    #[test]
    fn config_disable_switches_detector_off() {
        let cfg = PatternConfig::parse(
            r#"
            [patterns]
            disable = ["NoStaticState", "DI"]
            "#,
        )
        .unwrap();
        assert!(!cfg.enabled(Pattern::NoStaticState));
        assert!(!cfg.enabled(Pattern::DiOnly));
        assert!(cfg.enabled(Pattern::Layered));
    }

    #[test]
    fn config_overrides_layer_order_and_matchers() {
        let cfg = PatternConfig::parse(
            r#"
            [layers]
            order = ["ui", "service", "repository"]
            [layers.package_matchers]
            ui = "*.ui|*.web"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.layer_order, vec!["ui", "service", "repository"]);
        assert_eq!(cfg.layer_of("com.x.ui.Screen"), Some("ui"));
        // Default matchers for untouched layers survive the overlay.
        assert_eq!(cfg.layer_of("com.x.service.Foo"), Some("service"));
    }

    #[test]
    fn config_missing_file_is_default() {
        let cfg = PatternConfig::load(Path::new("/nonexistent-repo-xyz"));
        assert_eq!(cfg, PatternConfig::default());
    }

    #[test]
    fn check_all_skips_disabled_patterns() {
        let repo = mk_repo(vec![make_module("m", vec![cls("a.b.C")])]);
        let cfg = PatternConfig::parse(
            "[patterns]\ndisable = [\"Repository\", \"DI\", \"Layered\", \"Transactional\"]\n",
        )
        .unwrap();
        let results = check_all(&repo, &Scope::default(), &cfg);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].pattern, Pattern::NoStaticState);
    }

    #[test]
    fn visible_violations_hide_low_confidence() {
        let res = PatternResult {
            pattern: Pattern::DiOnly,
            holds: Vec::new(),
            violations: vec![
                Violation {
                    module: "m".into(),
                    file: PathBuf::from("A.java"),
                    line: 1,
                    fqn: "a.A".into(),
                    message: "loud".into(),
                    severity: 2,
                    confidence: 0.9,
                },
                Violation {
                    module: "m".into(),
                    file: PathBuf::from("B.java"),
                    line: 1,
                    fqn: "a.B".into(),
                    message: "quiet".into(),
                    severity: 2,
                    confidence: 0.4,
                },
            ],
            confidence: 0.8,
        };
        let visible = res.visible_violations();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].message, "loud");
    }
}
