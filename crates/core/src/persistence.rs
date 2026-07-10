// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Per-repo persistence configuration + backend resolution (Issues #115, #116).
//!
//! A repository may carry a `.projectmind/config.toml` selecting which
//! persistence backend each data class uses:
//!
//! ```toml
//! [persistence.annotations]
//! backend = "json"            # default — the only annotation backend today
//!
//! [persistence.code_graph]
//! backend = "sqlite"          # default "none"; also "memory"
//! path = ".projectmind/graph.db"  # optional, sqlite only; relative = repo-relative
//! ```
//!
//! Discovery order: `<repo>/.projectmind/config.toml`, then
//! `$XDG_CONFIG_HOME/projectmind/defaults.toml` (a machine-wide default
//! for repos without their own file), then built-in defaults. The files
//! are *not* merged — the first one found wins whole.
//!
//! Error policy (per #115/#116):
//!
//! - **Missing file** → built-in defaults. Zero-config keeps today's
//!   behavior exactly: JSON annotations, no code-graph cache.
//! - **Malformed TOML** → [`PersistenceConfig::load`] returns a loud
//!   error; [`resolve_stores`] logs it and falls back to the defaults so
//!   a typo can never brick opening a repo.
//! - **Unknown keys** → collected as warnings (logged, never fatal), so
//!   a config written by a newer ProjectMind still opens here.
//! - **Unknown backend name** → an actionable error from
//!   [`resolve_stores`]: the user explicitly asked for something this
//!   build cannot provide, silently ignoring that would be worse.

use std::path::{Path, PathBuf};

use projectmind_plugin_api::storage::{AnnotationStore, CodeGraphStore};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::annotations::JsonAnnotationStore;
use crate::code_graph::MemoryCodeGraphStore;
use crate::code_graph_sqlite::SqliteCodeGraphStore;

/// Directory inside the repo that holds ProjectMind files.
pub const CONFIG_DIR: &str = ".projectmind";
/// Config file name inside [`CONFIG_DIR`].
pub const CONFIG_FILE: &str = "config.toml";

/// Root shape of `config.toml`. Persistence settings live under a
/// `[persistence.*]` namespace so future non-persistence settings can
/// share the file without a breaking change.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    persistence: PersistenceConfig,
}

/// The `[persistence]` section: backend selection per data class.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PersistenceConfig {
    /// `[persistence.annotations]`
    #[serde(default)]
    pub annotations: AnnotationsSection,
    /// `[persistence.code_graph]`
    #[serde(default)]
    pub code_graph: CodeGraphSection,
}

/// Backend selection for user annotations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationsSection {
    /// Backend id. Supported: `"json"` (default).
    ///
    /// Kept as a free string (not an enum) so an unsupported value
    /// survives parsing and gets an actionable resolver error instead
    /// of a generic TOML type error.
    #[serde(default = "default_annotations_backend")]
    pub backend: String,
}

impl Default for AnnotationsSection {
    fn default() -> Self {
        Self {
            backend: default_annotations_backend(),
        }
    }
}

fn default_annotations_backend() -> String {
    "json".to_string()
}

/// Backend selection for the code-graph cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeGraphSection {
    /// Backend id. Supported: `"none"` (default), `"memory"`, `"sqlite"`.
    #[serde(default = "default_code_graph_backend")]
    pub backend: String,
    /// SQLite only: override for the database file. A relative path is
    /// resolved against the repo root. Default:
    /// [`SqliteCodeGraphStore::default_cache_path`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl Default for CodeGraphSection {
    fn default() -> Self {
        Self {
            backend: default_code_graph_backend(),
            path: None,
        }
    }
}

fn default_code_graph_backend() -> String {
    "none".to_string()
}

/// A successfully loaded (or defaulted) configuration.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    /// The effective configuration.
    pub config: PersistenceConfig,
    /// File the config came from; `None` = built-in defaults.
    pub source: Option<PathBuf>,
    /// Non-fatal findings (unknown keys), ready for logging.
    pub warnings: Vec<String>,
}

/// Errors from loading/parsing `config.toml`.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file exists but could not be read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// Offending file.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// The file is not valid TOML / does not match the schema.
    #[error("malformed {path}: {message}")]
    Malformed {
        /// Offending file.
        path: PathBuf,
        /// Parser diagnostics.
        message: String,
    },
}

impl PersistenceConfig {
    /// Location of the per-repo config file.
    #[must_use]
    pub fn config_path(repo_root: &Path) -> PathBuf {
        repo_root.join(CONFIG_DIR).join(CONFIG_FILE)
    }

    /// Machine-wide fallback (`$XDG_CONFIG_HOME/projectmind/defaults.toml`
    /// on Linux, the platform config dir elsewhere).
    #[must_use]
    pub fn user_defaults_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("projectmind").join("defaults.toml"))
    }

    /// Load the effective config for `repo_root`. Missing files are not
    /// an error (built-in defaults apply); a malformed file is.
    pub fn load(repo_root: &Path) -> Result<LoadedConfig, ConfigError> {
        Self::load_with_fallback(
            &Self::config_path(repo_root),
            Self::user_defaults_path().as_deref(),
        )
    }

    /// Like [`PersistenceConfig::load`] but with an explicit fallback
    /// path — the testable core of the discovery order.
    fn load_with_fallback(
        repo_config: &Path,
        fallback: Option<&Path>,
    ) -> Result<LoadedConfig, ConfigError> {
        for candidate in std::iter::once(repo_config).chain(fallback) {
            match std::fs::read_to_string(candidate) {
                Ok(text) => {
                    let (config, warnings) =
                        Self::parse(&text).map_err(|message| ConfigError::Malformed {
                            path: candidate.to_path_buf(),
                            message,
                        })?;
                    return Ok(LoadedConfig {
                        config,
                        source: Some(candidate.to_path_buf()),
                        warnings,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(ConfigError::Io {
                        path: candidate.to_path_buf(),
                        source: e,
                    })
                }
            }
        }
        Ok(LoadedConfig {
            config: PersistenceConfig::default(),
            source: None,
            warnings: Vec::new(),
        })
    }

    /// Parse a config document. Returns the config plus warnings for
    /// unknown keys (which are tolerated, per #115: "unknown keys
    /// warned"). A syntactically broken document is an `Err` with the
    /// TOML diagnostics.
    pub fn parse(text: &str) -> Result<(Self, Vec<String>), String> {
        let value: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
        let warnings = unknown_key_warnings(&value);
        let file: ConfigFile = value.try_into().map_err(|e| e.to_string())?;
        Ok((file.persistence, warnings))
    }
}

/// Walk the parsed document and flag keys the schema doesn't know.
/// Tolerant by design — a newer client may have written them — but a
/// warning makes typos (`backened = …`) visible instead of silently
/// falling back to defaults.
fn unknown_key_warnings(value: &toml::Value) -> Vec<String> {
    let mut warnings = Vec::new();
    let known: &[(&str, &[&str])] = &[
        ("", &["persistence"]),
        ("persistence", &["annotations", "code_graph"]),
        ("persistence.annotations", &["backend"]),
        ("persistence.code_graph", &["backend", "path"]),
    ];
    for (prefix, keys) in known {
        let table = prefix
            .split('.')
            .filter(|s| !s.is_empty())
            .try_fold(value, |v, key| v.get(key));
        let Some(toml::Value::Table(table)) = table else {
            continue;
        };
        for key in table.keys() {
            if !keys.contains(&key.as_str()) {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                warnings.push(format!("unknown config key `{path}` (ignored)"));
            }
        }
    }
    warnings
}

/// The stores selected for a repository.
///
/// `annotations` is `None` only when opening the configured store
/// failed (logged); `code_graph` is `None` when no cache is configured
/// — the zero-config default, mirroring today's "parse fresh every
/// open" behavior.
#[derive(Debug)]
pub struct ResolvedStores {
    /// Store for user annotations.
    pub annotations: Option<Box<dyn AnnotationStore>>,
    /// Code-graph cache, when one is configured.
    pub code_graph: Option<Box<dyn CodeGraphStore>>,
    /// Backend id actually in effect for the code graph (`"memory"`,
    /// `"sqlite"`); `None` when no cache is active. Surfaced to the UI
    /// as diagnostics.
    pub code_graph_backend: Option<String>,
}

/// Errors from backend resolution.
#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    /// The config names a backend this build doesn't provide.
    #[error(
        "unsupported {section} backend {backend:?} in .projectmind/config.toml — \
         supported: {supported}. Fix the entry or remove it to use the default \
         ({default:?})."
    )]
    UnsupportedBackend {
        /// Config section (`annotations` / `code_graph`).
        section: &'static str,
        /// The rejected backend id.
        backend: String,
        /// Comma-separated list of supported ids.
        supported: &'static str,
        /// Default backend id for the section.
        default: &'static str,
    },
}

/// Resolve the persistence stores for `repo_root` (Issue #116).
///
/// Reads `.projectmind/config.toml` (see [`PersistenceConfig::load`])
/// and constructs the selected backends. A malformed config file is
/// downgraded to a warning + defaults — opening a repo must never fail
/// on a config typo. A *valid* config naming an unknown backend is an
/// error: the user asked for something we can't silently ignore.
pub fn resolve_stores(repo_root: &Path) -> Result<ResolvedStores, PersistenceError> {
    let loaded = match PersistenceConfig::load(repo_root) {
        Ok(loaded) => {
            for w in &loaded.warnings {
                warn!(config = ?loaded.source, "{w}");
            }
            loaded
        }
        Err(err) => {
            warn!(error = %err, "persistence config unusable; falling back to defaults");
            LoadedConfig {
                config: PersistenceConfig::default(),
                source: None,
                warnings: Vec::new(),
            }
        }
    };
    resolve_with(repo_root, &loaded.config)
}

/// Resolution core, decoupled from file discovery for tests.
pub fn resolve_with(
    repo_root: &Path,
    config: &PersistenceConfig,
) -> Result<ResolvedStores, PersistenceError> {
    let annotations: Option<Box<dyn AnnotationStore>> = match config.annotations.backend.as_str() {
        "json" => match JsonAnnotationStore::open(repo_root) {
            Ok(store) => Some(Box::new(store)),
            Err(err) => {
                warn!(error = %err, "failed to open annotations store; continuing without one");
                None
            }
        },
        other => {
            return Err(PersistenceError::UnsupportedBackend {
                section: "annotations",
                backend: other.to_string(),
                supported: "json",
                default: "json",
            })
        }
    };

    let (code_graph, code_graph_backend): (Option<Box<dyn CodeGraphStore>>, Option<String>) =
        match config.code_graph.backend.as_str() {
            "none" => (None, None),
            "memory" => (
                Some(Box::new(MemoryCodeGraphStore::new())),
                Some("memory".to_string()),
            ),
            "sqlite" => {
                let path = match &config.code_graph.path {
                    Some(p) if p.is_absolute() => p.clone(),
                    Some(p) => repo_root.join(p),
                    None => SqliteCodeGraphStore::default_cache_path(repo_root),
                };
                match SqliteCodeGraphStore::open(&path) {
                    Ok(store) => (Some(Box::new(store)), Some("sqlite".to_string())),
                    Err(err) => {
                        // The cache is disposable; never block a repo
                        // open because its file can't be created.
                        warn!(error = %err, path = %path.display(),
                              "failed to open sqlite code-graph cache; continuing without one");
                        (None, None)
                    }
                }
            }
            other => {
                return Err(PersistenceError::UnsupportedBackend {
                    section: "code_graph",
                    backend: other.to_string(),
                    supported: "none, memory, sqlite",
                    default: "none",
                })
            }
        };

    Ok(ResolvedStores {
        annotations,
        code_graph,
        code_graph_backend,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut p = std::env::temp_dir();
            p.push(format!("projectmind-cfg-test-{}-{}", std::process::id(), n));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write_repo_config(repo: &Path, text: &str) {
        let path = PersistenceConfig::config_path(repo);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    // ---- parsing (#115) ----------------------------------------------

    #[test]
    fn parse_full_document_roundtrips() {
        let (config, warnings) = PersistenceConfig::parse(
            r#"
            [persistence.annotations]
            backend = "json"

            [persistence.code_graph]
            backend = "sqlite"
            path = "cache/graph.db"
            "#,
        )
        .unwrap();
        assert!(warnings.is_empty());
        assert_eq!(config.annotations.backend, "json");
        assert_eq!(config.code_graph.backend, "sqlite");
        assert_eq!(
            config.code_graph.path.as_deref(),
            Some(Path::new("cache/graph.db"))
        );
    }

    #[test]
    fn empty_document_is_all_defaults() {
        let (config, warnings) = PersistenceConfig::parse("").unwrap();
        assert!(warnings.is_empty());
        assert_eq!(config, PersistenceConfig::default());
        assert_eq!(config.annotations.backend, "json");
        assert_eq!(config.code_graph.backend, "none");
        assert_eq!(config.code_graph.path, None);
    }

    #[test]
    fn malformed_toml_is_a_loud_error() {
        assert!(PersistenceConfig::parse("this is [not toml").is_err());
        // Type errors are loud too — a table where a string belongs.
        assert!(PersistenceConfig::parse("[persistence.annotations]\nbackend = 42").is_err());
    }

    #[test]
    fn unknown_keys_warn_but_do_not_fail() {
        let (config, warnings) = PersistenceConfig::parse(
            r#"
            surprise = true

            [persistence]
            future_section = "ok"

            [persistence.code_graph]
            backend = "memory"
            backened = "typo"
            "#,
        )
        .unwrap();
        assert_eq!(config.code_graph.backend, "memory");
        let joined = warnings.join("\n");
        assert!(joined.contains("`surprise`"), "got: {joined}");
        assert!(
            joined.contains("`persistence.future_section`"),
            "got: {joined}"
        );
        assert!(
            joined.contains("`persistence.code_graph.backened`"),
            "got: {joined}"
        );
    }

    // ---- discovery ---------------------------------------------------

    #[test]
    fn missing_files_yield_builtin_defaults() {
        let repo = TempDir::new();
        let loaded = PersistenceConfig::load_with_fallback(
            &PersistenceConfig::config_path(repo.path()),
            None,
        )
        .unwrap();
        assert_eq!(loaded.config, PersistenceConfig::default());
        assert_eq!(loaded.source, None);
    }

    #[test]
    fn repo_config_wins_over_user_defaults() {
        let repo = TempDir::new();
        let user = TempDir::new();
        write_repo_config(
            repo.path(),
            "[persistence.code_graph]\nbackend = \"memory\"\n",
        );
        let fallback = user.path().join("defaults.toml");
        std::fs::write(
            &fallback,
            "[persistence.code_graph]\nbackend = \"sqlite\"\n",
        )
        .unwrap();

        let loaded = PersistenceConfig::load_with_fallback(
            &PersistenceConfig::config_path(repo.path()),
            Some(&fallback),
        )
        .unwrap();
        assert_eq!(loaded.config.code_graph.backend, "memory");
        assert_eq!(
            loaded.source,
            Some(PersistenceConfig::config_path(repo.path()))
        );
    }

    #[test]
    fn user_defaults_apply_when_repo_has_no_config() {
        let repo = TempDir::new();
        let user = TempDir::new();
        let fallback = user.path().join("defaults.toml");
        std::fs::write(
            &fallback,
            "[persistence.code_graph]\nbackend = \"memory\"\n",
        )
        .unwrap();

        let loaded = PersistenceConfig::load_with_fallback(
            &PersistenceConfig::config_path(repo.path()),
            Some(&fallback),
        )
        .unwrap();
        assert_eq!(loaded.config.code_graph.backend, "memory");
        assert_eq!(loaded.source, Some(fallback));
    }

    // ---- resolution (#116) ---------------------------------------------

    #[test]
    fn no_config_resolves_to_todays_defaults() {
        let repo = TempDir::new();
        let resolved = resolve_stores(repo.path()).unwrap();
        assert!(
            resolved.annotations.is_some(),
            "JSON annotations by default"
        );
        assert!(
            resolved.code_graph.is_none(),
            "no code-graph cache by default"
        );
        assert_eq!(resolved.code_graph_backend, None);
        // Zero-config must not litter the repo with a .projectmind dir.
        assert!(!repo.path().join(CONFIG_DIR).exists());
    }

    #[test]
    fn explicit_sqlite_backend_creates_the_cache_at_the_override_path() {
        let repo = TempDir::new();
        write_repo_config(
            repo.path(),
            "[persistence.code_graph]\nbackend = \"sqlite\"\npath = \"cache/graph.db\"\n",
        );
        let resolved = resolve_stores(repo.path()).unwrap();
        assert!(resolved.code_graph.is_some());
        assert_eq!(resolved.code_graph_backend.as_deref(), Some("sqlite"));
        assert!(
            repo.path().join("cache/graph.db").exists(),
            "relative path resolves against the repo root"
        );
    }

    #[test]
    fn explicit_memory_backend_resolves() {
        let repo = TempDir::new();
        write_repo_config(
            repo.path(),
            "[persistence.code_graph]\nbackend = \"memory\"\n",
        );
        let resolved = resolve_stores(repo.path()).unwrap();
        assert!(resolved.code_graph.is_some());
        assert_eq!(resolved.code_graph_backend.as_deref(), Some("memory"));
    }

    #[test]
    fn unknown_code_graph_backend_is_an_actionable_error() {
        let repo = TempDir::new();
        write_repo_config(
            repo.path(),
            "[persistence.code_graph]\nbackend = \"surrealdb\"\n",
        );
        let err = resolve_stores(repo.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("surrealdb"), "names the offender: {msg}");
        assert!(msg.contains("none, memory, sqlite"), "lists options: {msg}");
        assert!(msg.contains("config.toml"), "points at the file: {msg}");
    }

    #[test]
    fn unknown_annotations_backend_is_an_actionable_error() {
        let repo = TempDir::new();
        write_repo_config(
            repo.path(),
            "[persistence.annotations]\nbackend = \"sqlite\"\n",
        );
        let err = resolve_stores(repo.path()).unwrap_err();
        assert!(err.to_string().contains("annotations"));
        assert!(err.to_string().contains("json"));
    }

    #[test]
    fn malformed_config_falls_back_to_defaults_without_crashing() {
        let repo = TempDir::new();
        write_repo_config(repo.path(), "this is [not toml");
        let resolved = resolve_stores(repo.path()).unwrap();
        assert!(resolved.annotations.is_some());
        assert!(resolved.code_graph.is_none());
    }

    #[test]
    fn resolved_stores_are_usable_through_the_traits() {
        use projectmind_plugin_api::storage::{GraphNode, GraphQuery};

        let repo = TempDir::new();
        write_repo_config(
            repo.path(),
            "[persistence.code_graph]\nbackend = \"sqlite\"\npath = \"g.db\"\n",
        );
        let mut resolved = resolve_stores(repo.path()).unwrap();

        let store = resolved.code_graph.as_mut().unwrap();
        store
            .upsert_node(GraphNode {
                id: 0,
                kind: "class".into(),
                label: "Alpha".into(),
                properties: serde_json::Map::new(),
            })
            .unwrap();
        assert_eq!(store.query(&GraphQuery::default()).unwrap().len(), 1);

        let ann = resolved.annotations.as_ref().unwrap();
        assert!(ann.all().unwrap().is_empty());
    }
}
