// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Optional LAN browser host for ProjectMind.
//!
//! This crate is deliberately self-contained so browser mode can be removed
//! without touching the core parser, language plugins, or the Tauri shell.

#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use projectmind_core::files::{self, MarkdownFile, MarkdownHit, ModuleFile};
use projectmind_core::git::{self, ChangedFile};
use projectmind_core::html::{self, HtmlFile, HtmlSnippet};
use projectmind_core::state::{self, UiState, ViewIntent};
use projectmind_core::walkthrough::{
    self as wt, FeedbackEvent, FeedbackKind, FeedbackLog, Walkthrough,
};
use projectmind_core::{diagram, Engine, Repository};
use projectmind_framework_lombok::LombokPlugin;
use projectmind_framework_spring::SpringPlugin;
use projectmind_lang_java::JavaPlugin;
use projectmind_lang_rust::RustPlugin;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Total cap on request-line + headers combined. Anything above this is rejected as 413.
const MAX_HEADER_BYTES: usize = 16 * 1024;
/// Cap on body bytes. API payloads are tiny JSON; 1 MB is generous for a LAN debug surface.
const MAX_BODY_BYTES: usize = 1024 * 1024;

/// Parsed HTTP request as needed by the router.
#[derive(Debug)]
struct Request {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

/// Errors returned by [`parse_request`].
#[derive(Debug)]
enum ParseError {
    /// Header bytes exceeded `MAX_HEADER_BYTES` or `Content-Length` exceeded `MAX_BODY_BYTES`.
    PayloadTooLarge,
    /// Request line was empty or had fewer than two whitespace-separated parts.
    Malformed,
    /// Underlying I/O error while reading from the socket.
    Io(std::io::Error),
}

impl From<std::io::Error> for ParseError {
    fn from(value: std::io::Error) -> Self {
        ParseError::Io(value)
    }
}

/// Read an HTTP/1.x request from `reader` with hard caps on header and body size.
///
/// The reader is wrapped in `take((MAX_HEADER_BYTES + MAX_BODY_BYTES) as u64)` so a
/// malformed unbounded stream cannot exhaust memory even if the size accounting below
/// has a bug. The header loop additionally tracks total header bytes consumed and
/// returns [`ParseError::PayloadTooLarge`] if the tally would exceed `MAX_HEADER_BYTES`.
/// `Content-Length` is parsed and validated against `MAX_BODY_BYTES` BEFORE allocating
/// the body buffer.
fn parse_request<R: Read>(reader: R) -> Result<Request, ParseError> {
    let cap = (MAX_HEADER_BYTES + MAX_BODY_BYTES) as u64;
    let mut reader = BufReader::new(reader.take(cap));

    let mut header_bytes: usize = 0;

    let mut first = String::new();
    let n = reader.read_line(&mut first)?;
    header_bytes = header_bytes.saturating_add(n);
    if header_bytes > MAX_HEADER_BYTES {
        return Err(ParseError::PayloadTooLarge);
    }
    let parts: Vec<&str> = first.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(ParseError::Malformed);
    }
    let method = parts[0].to_string();
    let target = parts[1].to_string();

    let mut headers = BTreeMap::new();
    let mut content_len: usize = 0;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        header_bytes = header_bytes.saturating_add(n);
        if header_bytes > MAX_HEADER_BYTES {
            return Err(ParseError::PayloadTooLarge);
        }
        // EOF before reaching the blank line that terminates the header section.
        if n == 0 {
            return Err(ParseError::Malformed);
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                match value.parse::<usize>() {
                    Ok(v) => {
                        if v > MAX_BODY_BYTES {
                            return Err(ParseError::PayloadTooLarge);
                        }
                        content_len = v;
                    }
                    Err(_) => content_len = 0,
                }
            }
            headers.insert(name, value);
        }
    }

    let mut body = vec![0_u8; content_len];
    if content_len > 0 {
        reader.read_exact(&mut body)?;
    }

    Ok(Request {
        method,
        target,
        headers,
        body,
    })
}

/// Browser host startup configuration.
#[derive(Debug, Clone)]
pub struct BrowserHostConfig {
    /// Repository to open immediately.
    pub repo_root: Option<PathBuf>,
    /// Port to bind. Use `0` to let the OS pick a free port.
    pub port: u16,
    /// Directory containing the built Svelte frontend (`index.html`, assets).
    pub asset_dir: PathBuf,
    /// Open the default browser after startup.
    pub open_browser: bool,
}

/// Public status returned by MCP tools.
#[derive(Debug, Clone, Serialize)]
pub struct BrowserHostStatus {
    /// Bound socket address.
    pub bind: SocketAddr,
    /// Browser access URLs with the token in the fragment.
    pub urls: Vec<String>,
    /// Opened repository, if any.
    pub repo_root: Option<PathBuf>,
    /// Random session token.
    pub token: String,
}

#[derive(Debug)]
struct HostState {
    engine: Engine,
    repo: Option<Repository>,
    repo_root: Option<PathBuf>,
}

impl HostState {
    fn new() -> Self {
        let mut engine = Engine::new();
        engine.register_language(Box::new(JavaPlugin::new()));
        engine.register_language(Box::new(RustPlugin::new()));
        engine.register_framework(Box::new(SpringPlugin::new()));
        engine.register_framework(Box::new(LombokPlugin::new()));
        Self {
            engine,
            repo: None,
            repo_root: None,
        }
    }
}

#[derive(Debug)]
struct RunningHost {
    status: BrowserHostStatus,
    shared: Arc<Mutex<HostState>>,
    running: Arc<AtomicBool>,
}

static HOST: OnceLock<Mutex<Option<RunningHost>>> = OnceLock::new();

fn host_slot() -> &'static Mutex<Option<RunningHost>> {
    HOST.get_or_init(|| Mutex::new(None))
}

/// Start the LAN browser host, or return the existing host status.
///
/// The server binds to `0.0.0.0` by design because this mode is explicitly
/// for LAN/VM access. Every API endpoint requires the random bearer token.
pub fn start(config: BrowserHostConfig) -> anyhow::Result<BrowserHostStatus> {
    {
        let mut slot = host_slot().lock().expect("browser host slot poisoned");
        if let Some(existing) = slot.as_mut() {
            if let Some(root) = config.repo_root.as_ref() {
                let mut guard = existing.shared.lock().expect("browser host state poisoned");
                let summary = open_repo_locked(&mut guard, root)?;
                existing.status.repo_root = Some(summary.root);
            }
            if config.open_browser {
                if let Some(url) = existing.status.urls.first() {
                    if let Err(err) = open::that(url) {
                        tracing::warn!(error = %err, "failed to open browser");
                    }
                }
            }
            return Ok(existing.status.clone());
        }
    }

    let bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), config.port);
    let listener = TcpListener::bind(bind)?;
    let actual = listener.local_addr()?;
    let token = generate_token();
    let urls = access_urls(actual.port(), &token);
    let shared = Arc::new(Mutex::new(HostState::new()));
    let running = Arc::new(AtomicBool::new(true));

    if let Some(root) = config.repo_root.as_ref() {
        let mut guard = shared.lock().expect("browser host state poisoned");
        open_repo_locked(&mut guard, root)?;
    }

    let status = BrowserHostStatus {
        bind: actual,
        urls,
        repo_root: config.repo_root.clone(),
        token: token.clone(),
    };
    {
        let mut slot = host_slot().lock().expect("browser host slot poisoned");
        *slot = Some(RunningHost {
            status: status.clone(),
            shared: Arc::clone(&shared),
            running: Arc::clone(&running),
        });
    }

    let asset_dir = config.asset_dir;
    thread::spawn(move || serve(listener, shared, asset_dir, token, running));

    if config.open_browser {
        if let Some(url) = status.urls.first() {
            if let Err(err) = open::that(url) {
                tracing::warn!(error = %err, "failed to open browser");
            }
        }
    }

    Ok(status)
}

/// Return the current browser host status, if one has been started.
pub fn status() -> Option<BrowserHostStatus> {
    host_slot()
        .lock()
        .expect("browser host slot poisoned")
        .as_ref()
        .map(|h| h.status.clone())
}

/// Stop the host listener and forget the host status.
pub fn stop() {
    let mut slot = host_slot().lock().expect("browser host slot poisoned");
    if let Some(host) = slot.as_ref() {
        host.running.store(false, Ordering::Relaxed);
    }
    *slot = None;
}

fn serve(
    listener: TcpListener,
    shared: Arc<Mutex<HostState>>,
    asset_dir: PathBuf,
    token: String,
    running: Arc<AtomicBool>,
) {
    if let Err(err) = listener.set_nonblocking(true) {
        tracing::debug!(error = %err, "browser host nonblocking setup failed");
        return;
    }
    while running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                if !running.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(err) = stream.set_nonblocking(false) {
                    tracing::debug!(error = %err, "browser request blocking setup failed");
                    continue;
                }
                let shared = Arc::clone(&shared);
                let asset_dir = asset_dir.clone();
                let token = token.clone();
                thread::spawn(move || {
                    if let Err(err) = handle(stream, shared, &asset_dir, &token) {
                        tracing::debug!(error = %err, "browser request failed");
                    }
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(err) => tracing::debug!(error = %err, "browser host accept failed"),
        }
    }
}

fn handle(
    mut stream: TcpStream,
    shared: Arc<Mutex<HostState>>,
    asset_dir: &Path,
    token: &str,
) -> anyhow::Result<()> {
    let request = match parse_request(stream.try_clone()?) {
        Ok(r) => r,
        Err(ParseError::PayloadTooLarge) => {
            return json_response(&mut stream, 413, json!({"error": "request too large"}));
        }
        Err(ParseError::Malformed) => {
            return json_response(&mut stream, 400, json!({"error": "malformed request"}));
        }
        Err(ParseError::Io(err)) => return Err(err.into()),
    };
    let Request {
        method,
        target,
        headers,
        body,
    } = request;

    let (path, query) = split_target(&target);
    if path.starts_with("/api/") {
        if !authorized(&headers, token) {
            return json_response(
                &mut stream,
                401,
                json!({"error": "invalid or missing token"}),
            );
        }
        if method == "GET" && path == "/api/read_file_bytes" {
            let result = read_file_bytes_response(&mut stream, &query, shared);
            if let Err(err) = result {
                json_response(&mut stream, 400, json!({"error": err.to_string()}))?;
            }
            return Ok(());
        }
        let result = route_api(&method, &path, &query, &body, shared);
        match result {
            Ok(value) => json_response(&mut stream, 200, value),
            Err(err) => json_response(&mut stream, 400, json!({"error": err.to_string()})),
        }?;
    } else {
        serve_asset(&mut stream, asset_dir, &path)?;
    }
    Ok(())
}

fn route_api(
    method: &str,
    path: &str,
    query: &BTreeMap<String, String>,
    body: &[u8],
    shared: Arc<Mutex<HostState>>,
) -> anyhow::Result<Value> {
    let mut guard = shared.lock().expect("browser host state poisoned");
    match (method, path) {
        ("GET", "/api/current_state") => Ok(serde_json::to_value(state::read().ok().flatten())?),
        ("POST", "/api/open_repo") => {
            let args: PathArg = serde_json::from_slice(body)?;
            let summary = open_repo_locked(&mut guard, Path::new(&args.path))?;
            Ok(serde_json::to_value(summary)?)
        }
        ("GET", "/api/list_classes") => {
            let stereotype = query.get("stereotype").map(String::as_str);
            let module = query.get("module").map(String::as_str);
            Ok(serde_json::to_value(list_classes_locked(
                &guard, stereotype, module,
            )?)?)
        }
        ("GET", "/api/list_modules") => Ok(serde_json::to_value(list_modules_locked(&guard)?)?),
        ("GET", "/api/show_class") => {
            let fqn = required(query, "fqn")?;
            Ok(serde_json::to_value(show_class_locked(&guard, fqn)?)?)
        }
        ("GET", "/api/class_outline") => {
            let fqn = required(query, "fqn")?;
            Ok(serde_json::to_value(class_outline_locked(&guard, fqn)?)?)
        }
        ("GET", "/api/list_changes_since") => {
            let reference = required(query, "reference")?;
            let to = query.get("to").map(String::as_str);
            let repo = repo(&guard)?;
            Ok(serde_json::to_value(git::list_changes_since(
                &repo.root, reference, to,
            )?)?)
        }
        ("GET", "/api/file_recency") => {
            let repo = repo(&guard)?;
            Ok(serde_json::to_value(git::file_recency(&repo.root)?)?)
        }
        ("GET", "/api/show_diagram") => {
            let kind = required(query, "kind")?;
            let repo = repo(&guard)?;
            let spring = SpringPlugin::new();
            match kind {
                "bean-graph" => Ok(json!(diagram::render_bean_graph(repo, &spring))),
                "package-tree" => Ok(json!(diagram::render_package_tree(repo))),
                "folder-map" => Ok(json!(diagram::render_folder_map(repo))),
                other => anyhow::bail!("unknown diagram kind: {other}"),
            }
        }
        ("GET", "/api/show_diff") => {
            let reference = required(query, "reference")?;
            let to = query.get("to").map(String::as_str);
            let repo = repo(&guard)?;
            Ok(json!(git::unified_diff(&repo.root, reference, to)?))
        }
        ("GET", "/api/read_file_text") => {
            let path = required(query, "path")?;
            Ok(json!(read_file_text_locked(&guard, Path::new(path))?))
        }
        ("GET", "/api/list_markdown_files") => {
            let root = required(query, "root")?;
            ensure_under_repo(&guard, Path::new(root))?;
            Ok(serde_json::to_value(files::list_markdown_files(
                Path::new(root),
            ))?)
        }
        ("GET", "/api/search_markdown") => {
            let root = required(query, "root")?;
            ensure_under_repo(&guard, Path::new(root))?;
            let q = query.get("query").map_or("", String::as_str);
            let limit = query
                .get("limit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(200);
            Ok(serde_json::to_value(files::search_markdown(
                Path::new(root),
                q,
                limit,
            ))?)
        }
        ("GET", "/api/list_html_files") => {
            let root = required(query, "root")?;
            ensure_under_repo(&guard, Path::new(root))?;
            Ok(serde_json::to_value(html::list_html_files(Path::new(
                root,
            )))?)
        }
        ("GET", "/api/find_html_snippets") => {
            let root = required(query, "root")?;
            ensure_under_repo(&guard, Path::new(root))?;
            Ok(serde_json::to_value(html::find_html_snippets(Path::new(
                root,
            )))?)
        }
        ("GET", "/api/list_module_files") => {
            let module_id = required(query, "module")?;
            let repo = repo(&guard)?;
            let module = repo
                .modules
                .get(module_id)
                .ok_or_else(|| anyhow::anyhow!("module not found: {module_id}"))?;
            // Defensive: even though the module root came from the parsed repo,
            // the `ensure_under_repo` check guarantees we never walk somewhere
            // unexpected if the parser ever ends up with a stray absolute path.
            ensure_under_repo(&guard, &module.root)?;
            Ok(serde_json::to_value(files::list_module_files(
                &module.root,
                &["pdf", "png", "jpg", "jpeg", "webp", "gif"],
            ))?)
        }
        ("GET", "/api/current_walkthrough") => Ok(serde_json::to_value(wt::read_body()?)?),
        ("GET", "/api/current_walkthrough_feedback") => Ok(serde_json::to_value(
            wt::read_feedback().unwrap_or_default(),
        )?),
        ("POST", "/api/walkthrough_ack") => {
            let args: FeedbackArgs = serde_json::from_slice(body)?;
            let event = FeedbackEvent {
                walkthrough_id: args.walkthrough_id,
                step: args.step,
                kind: FeedbackKind::Understood,
                comment: None,
                ts: now_secs(),
            };
            Ok(serde_json::to_value(wt::append_feedback(event)?)?)
        }
        ("POST", "/api/walkthrough_request_more") => {
            let args: FeedbackArgs = serde_json::from_slice(body)?;
            let event = FeedbackEvent {
                walkthrough_id: args.walkthrough_id,
                step: args.step,
                kind: FeedbackKind::MoreDetail,
                comment: args.comment,
                ts: now_secs(),
            };
            Ok(serde_json::to_value(wt::append_feedback(event)?)?)
        }
        ("POST", "/api/set_walkthrough_step") => {
            let args: StepArgs = serde_json::from_slice(body)?;
            let prev = state::read().ok().flatten().unwrap_or_default();
            state::write(UiState {
                repo_root: prev.repo_root,
                view: ViewIntent::Walkthrough {
                    id: args.id,
                    step: args.step,
                },
                ..UiState::default()
            })?;
            Ok(json!({ "ok": true }))
        }
        ("POST", "/api/end_walkthrough") => {
            wt::clear()?;
            let prev = state::read().ok().flatten().unwrap_or_default();
            state::write(UiState {
                repo_root: prev.repo_root,
                view: ViewIntent::default(),
                ..UiState::default()
            })?;
            Ok(json!({ "ok": true }))
        }
        _ => anyhow::bail!("unknown endpoint: {method} {path}"),
    }
}

#[derive(Deserialize)]
struct PathArg {
    path: String,
}

#[derive(Deserialize)]
struct FeedbackArgs {
    walkthrough_id: String,
    step: u32,
    #[serde(default)]
    comment: Option<String>,
}

#[derive(Deserialize)]
struct StepArgs {
    id: String,
    step: u32,
}

/// Public summary of an opened repository.
#[derive(Debug, Serialize)]
pub struct RepoSummary {
    /// Repository root.
    pub root: PathBuf,
    /// Module count.
    pub modules: usize,
    /// Class count.
    pub classes: usize,
    /// Active language plugin ids.
    pub language_plugins: Vec<&'static str>,
    /// Active framework plugin ids.
    pub framework_plugins: Vec<&'static str>,
    /// Markdown file count.
    pub markdown_count: usize,
    /// HTML file/snippet count.
    pub html_count: usize,
    /// Diagram kinds available for this repo + plugin set. The browser UI
    /// uses this to render Diagram-tab buttons dynamically.
    pub available_diagrams: Vec<String>,
    /// Top-level UI tabs the active plugin set contributes for this repo.
    /// Core ships `files` + `diagrams`; plugins can append more (a future
    /// `framework-junit` "Tests" tab, for example). The frontend renders
    /// one nav button per entry.
    pub tabs: Vec<projectmind_core::TabDescriptor>,
}

/// One class entry exposed to the browser UI.
#[derive(Debug, Serialize)]
pub struct ClassEntry {
    /// Fully-qualified class name.
    pub fqn: String,
    /// Simple class name.
    pub name: String,
    /// Path relative to the module root.
    pub file: PathBuf,
    /// Stereotypes.
    pub stereotypes: Vec<String>,
    /// Lowercase class kind.
    pub kind: String,
    /// Module id.
    pub module: String,
}

/// Per-module summary for the browser UI.
#[derive(Debug, Serialize)]
pub struct ModuleEntry {
    /// Module id.
    pub id: String,
    /// Module display name.
    pub name: String,
    /// Module root.
    pub root: PathBuf,
    /// Class count.
    pub classes: usize,
    /// Stereotype histogram.
    pub stereotypes: BTreeMap<String, u32>,
}

/// Detailed class data with source code.
#[derive(Debug, Serialize)]
pub struct ClassDetails {
    /// Fully-qualified class name.
    pub fqn: String,
    /// Path relative to the module root.
    pub file: PathBuf,
    /// First line.
    pub line_start: u32,
    /// Last line.
    pub line_end: u32,
    /// UTF-8 source.
    pub source: String,
}

/// Structural outline of a class — methods, fields, annotations, no source.
/// Mirror of the Tauri-side `ClassOutline`. Used by the GUI's `ClassViewer`
/// to render a side-panel with click-to-jump navigation.
#[derive(Debug, Serialize)]
pub struct ClassOutline {
    /// Fully-qualified class name.
    pub fqn: String,
    /// Simple class name.
    pub name: String,
    /// Lowercase class kind (`class`, `interface`, `enum`, `record`, `annotation`).
    pub kind: String,
    /// Visibility (`public`, `protected`, `package`, `private`).
    pub visibility: String,
    /// First line of the class definition (1-based).
    pub line_start: u32,
    /// Last line of the class definition.
    pub line_end: u32,
    /// Stereotypes attached by framework plugins.
    pub stereotypes: Vec<String>,
    /// Class-level annotation names (without `@`).
    pub annotations: Vec<String>,
    /// Methods, in source order.
    pub methods: Vec<MethodOutline>,
    /// Fields, in source order.
    pub fields: Vec<FieldOutline>,
    /// Declared parent types: `extends` then `implements` / trait-impl
    /// targets. Drives the inheritance crumb in the GUI header.
    pub super_types: Vec<SuperTypeOutline>,
}

/// One declared parent type for the class outline.
#[derive(Debug, Serialize)]
pub struct SuperTypeOutline {
    /// Type name as written in source.
    pub name: String,
    /// `"extends"` or `"implements"`.
    pub kind: String,
}

/// One method entry in the class outline.
#[derive(Debug, Serialize)]
pub struct MethodOutline {
    /// Method name.
    pub name: String,
    /// Visibility.
    pub visibility: String,
    /// Whether the method is static.
    pub is_static: bool,
    /// 1-based start line of the method definition.
    pub line_start: u32,
    /// 1-based end line of the method definition.
    pub line_end: u32,
    /// Annotation names.
    pub annotations: Vec<String>,
}

/// One field entry in the class outline.
#[derive(Debug, Serialize)]
pub struct FieldOutline {
    /// Field name.
    pub name: String,
    /// Field type as written in source.
    #[serde(rename = "type")]
    pub type_text: String,
    /// Visibility.
    pub visibility: String,
    /// Whether the field is static.
    pub is_static: bool,
    /// 1-based line where the field is declared.
    pub line: u32,
    /// Annotation names.
    pub annotations: Vec<String>,
}

fn open_repo_locked(state: &mut HostState, path: &Path) -> anyhow::Result<RepoSummary> {
    if !path.is_absolute() {
        anyhow::bail!("repo path must be absolute: {}", path.display());
    }
    let repo = state.engine.open_repo(path)?;
    let markdown_count = files::list_markdown_files(&repo.root).len();
    let html_count =
        html::list_html_files(&repo.root).len() + html::find_html_snippets(&repo.root).len();
    let available_diagrams = state.engine.available_diagrams(&repo);
    let tabs = state.engine.available_tabs(&repo);
    let summary = RepoSummary {
        root: repo.root.clone(),
        modules: repo.modules.len(),
        classes: repo.class_count(),
        language_plugins: state.engine.language_ids(),
        framework_plugins: state.engine.framework_ids(),
        markdown_count,
        html_count,
        available_diagrams,
        tabs,
    };
    state.repo_root = Some(repo.root.clone());
    state::write(UiState {
        repo_root: Some(repo.root.clone()),
        view: ViewIntent::default(),
        ..UiState::default()
    })?;
    state.repo = Some(repo);
    Ok(summary)
}

fn repo(state: &HostState) -> anyhow::Result<&Repository> {
    state
        .repo
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no repository open"))
}

fn list_classes_locked(
    state: &HostState,
    stereotype: Option<&str>,
    module: Option<&str>,
) -> anyhow::Result<Vec<ClassEntry>> {
    let repo = repo(state)?;
    let mut out = Vec::new();
    for (mod_id, m) in &repo.modules {
        if module.is_some_and(|target| target != mod_id) {
            continue;
        }
        for class in m.classes.values() {
            if stereotype.is_some_and(|s| !class.stereotypes.iter().any(|x| x == s)) {
                continue;
            }
            out.push(ClassEntry {
                fqn: class.fqn.clone(),
                name: class.name.clone(),
                file: class.file.clone(),
                stereotypes: class.stereotypes.clone(),
                kind: format!("{:?}", class.kind).to_lowercase(),
                module: mod_id.clone(),
            });
        }
    }
    Ok(out)
}

fn list_modules_locked(state: &HostState) -> anyhow::Result<Vec<ModuleEntry>> {
    let repo = repo(state)?;
    let mut out = Vec::new();
    for module in repo.modules.values() {
        let mut counts = BTreeMap::new();
        for class in module.classes.values() {
            for s in &class.stereotypes {
                *counts.entry(s.clone()).or_insert(0) += 1;
            }
        }
        out.push(ModuleEntry {
            id: module.id.clone(),
            name: module.name.clone(),
            root: module.root.clone(),
            classes: module.classes.len(),
            stereotypes: counts,
        });
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.classes));
    Ok(out)
}

fn show_class_locked(state: &HostState, fqn: &str) -> anyhow::Result<ClassDetails> {
    let repo = repo(state)?;
    let (module, class) = repo
        .find_class(fqn)
        .ok_or_else(|| anyhow::anyhow!("class not found: {fqn}"))?;
    let abs = module.root.join(&class.file);
    let source = std::fs::read_to_string(&abs)?;
    Ok(ClassDetails {
        fqn: class.fqn.clone(),
        file: class.file.clone(),
        line_start: class.line_start,
        line_end: class.line_end,
        source,
    })
}

fn class_outline_locked(state: &HostState, fqn: &str) -> anyhow::Result<ClassOutline> {
    let repo = repo(state)?;
    let (_module, class) = repo
        .find_class(fqn)
        .ok_or_else(|| anyhow::anyhow!("class not found: {fqn}"))?;
    Ok(build_class_outline(class))
}

fn visibility_str(v: projectmind_plugin_api::Visibility) -> String {
    use projectmind_plugin_api::Visibility;
    match v {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::PackagePrivate => "package",
        Visibility::Private => "private",
    }
    .to_string()
}

fn build_class_outline(class: &projectmind_plugin_api::Class) -> ClassOutline {
    ClassOutline {
        fqn: class.fqn.clone(),
        name: class.name.clone(),
        kind: format!("{:?}", class.kind).to_lowercase(),
        visibility: visibility_str(class.visibility),
        line_start: class.line_start,
        line_end: class.line_end,
        stereotypes: class.stereotypes.clone(),
        annotations: class.annotations.iter().map(|a| a.name.clone()).collect(),
        methods: class
            .methods
            .iter()
            .map(|m| MethodOutline {
                name: m.name.clone(),
                visibility: visibility_str(m.visibility),
                is_static: m.is_static,
                line_start: m.line_start,
                line_end: m.line_end,
                annotations: m.annotations.iter().map(|a| a.name.clone()).collect(),
            })
            .collect(),
        fields: class
            .fields
            .iter()
            .map(|f| FieldOutline {
                name: f.name.clone(),
                type_text: f.type_text.clone(),
                visibility: visibility_str(f.visibility),
                is_static: f.is_static,
                line: f.line,
                annotations: f.annotations.iter().map(|a| a.name.clone()).collect(),
            })
            .collect(),
        super_types: class
            .super_types
            .iter()
            .map(|t| SuperTypeOutline {
                name: t.name.clone(),
                kind: match t.kind {
                    projectmind_plugin_api::TypeRefKind::Extends => "extends".to_string(),
                    projectmind_plugin_api::TypeRefKind::Implements => "implements".to_string(),
                },
            })
            .collect(),
    }
}

fn read_file_text_locked(state: &HostState, path: &Path) -> anyhow::Result<String> {
    ensure_under_repo(state, path)?;
    let bytes = std::fs::read(path)?;
    if bytes.len() > 10_000_000 {
        anyhow::bail!("file too large ({} bytes; limit 10 MB)", bytes.len());
    }
    Ok(String::from_utf8(bytes)?)
}

fn read_file_bytes_response(
    stream: &mut TcpStream,
    query: &BTreeMap<String, String>,
    shared: Arc<Mutex<HostState>>,
) -> anyhow::Result<()> {
    let path = Path::new(required(query, "path")?);
    let guard = shared.lock().expect("browser host state poisoned");
    ensure_under_repo(&guard, path)?;
    let bytes = std::fs::read(path)?;
    if bytes.len() > 50_000_000 {
        anyhow::bail!("file too large ({} bytes; limit 50 MB)", bytes.len());
    }
    let ctype = content_type(path);
    write!(
        stream,
        "HTTP/1.1 200 OK\r\n\
         Content-Type: {ctype}\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         X-Frame-Options: SAMEORIGIN\r\n\
         Referrer-Policy: no-referrer\r\n\
         \r\n",
        bytes.len()
    )?;
    stream.write_all(&bytes)?;
    Ok(())
}

fn ensure_under_repo(state: &HostState, path: &Path) -> anyhow::Result<()> {
    let root = state
        .repo_root
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no repository open"))?;
    if !path.is_absolute() {
        anyhow::bail!("path must be absolute: {}", path.display());
    }
    let canonical = path.canonicalize()?;
    let canonical_root = root.canonicalize()?;
    if !canonical.starts_with(&canonical_root) {
        anyhow::bail!("path outside opened repo: {}", path.display());
    }
    Ok(())
}

fn authorized(headers: &BTreeMap<String, String>, token: &str) -> bool {
    headers
        .get("authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|v| v == token)
}

fn serve_asset(stream: &mut TcpStream, asset_dir: &Path, path: &str) -> anyhow::Result<()> {
    let rel = if path == "/" {
        "index.html".to_string()
    } else {
        path.trim_start_matches('/').to_string()
    };
    let candidate = asset_dir.join(&rel);
    let root = asset_dir.canonicalize()?;
    let file = candidate
        .canonicalize()
        .unwrap_or_else(|_| root.join("index.html"));
    let file = if file.starts_with(&root) && file.is_file() {
        file
    } else {
        root.join("index.html")
    };
    let bytes = std::fs::read(&file)?;
    let ctype = content_type(&file);
    write!(
        stream,
        "HTTP/1.1 200 OK\r\n\
         Content-Type: {ctype}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         X-Frame-Options: SAMEORIGIN\r\n\
         Referrer-Policy: no-referrer\r\n\
         \r\n",
        bytes.len()
    )?;
    stream.write_all(&bytes)?;
    Ok(())
}

fn json_response(stream: &mut TcpStream, status: u16, value: Value) -> anyhow::Result<()> {
    let body = serde_json::to_vec(&value)?;
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        413 => "Payload Too Large",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         X-Frame-Options: SAMEORIGIN\r\n\
         Referrer-Policy: no-referrer\r\n\
         \r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    Ok(())
}

fn split_target(target: &str) -> (String, BTreeMap<String, String>) {
    let (path, raw_query) = target.split_once('?').unwrap_or((target, ""));
    let mut query = BTreeMap::new();
    for pair in raw_query.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        query.insert(percent_decode(k), percent_decode(v));
    }
    (path.to_string(), query)
}

fn required<'a>(query: &'a BTreeMap<String, String>, key: &str) -> anyhow::Result<&'a str> {
    query
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing query parameter: {key}"))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(a * 16 + b);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn generate_token() -> String {
    use std::fmt::Write;
    let mut buf = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    let mut s = String::with_capacity(buf.len() * 2);
    for b in &buf {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn access_urls(port: u16, token: &str) -> Vec<String> {
    let mut urls = vec![format!("http://127.0.0.1:{port}/#token={token}")];
    for ip in lan_ips() {
        let url = format!("http://{ip}:{port}/#token={token}");
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls
}

fn lan_ips() -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    if let Ok(sock) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        if sock.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).is_ok() {
            if let Ok(SocketAddr::V4(addr)) = sock.local_addr() {
                let ip = *addr.ip();
                if !ip.is_loopback() && !out.contains(&ip) {
                    out.push(ip);
                }
            }
        }
    }
    out
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[allow(dead_code, clippy::too_many_arguments)]
fn _type_check_public_payloads(
    _: MarkdownFile,
    _: MarkdownHit,
    _: ModuleFile,
    _: HtmlFile,
    _: HtmlSnippet,
    _: ChangedFile,
    _: Walkthrough,
    _: FeedbackLog,
) {
}

#[cfg(test)]
mod tests {
    use super::{
        authorized, content_type, parse_request, percent_decode, split_target, ParseError,
        MAX_BODY_BYTES, MAX_HEADER_BYTES,
    };
    use std::collections::BTreeMap;
    use std::io::Cursor;
    use std::path::Path;

    // ----- parse_request -----

    #[test]
    fn parse_request_get_no_body() {
        let raw = b"GET /api/foo HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = parse_request(Cursor::new(raw)).expect("parse ok");
        assert_eq!(req.method, "GET");
        assert_eq!(req.target, "/api/foo");
        assert_eq!(
            req.headers.get("host").map(String::as_str),
            Some("localhost")
        );
        assert!(req.body.is_empty());
    }

    #[test]
    fn parse_request_post_with_json_body() {
        let body = br#"{"path":"/tmp/foo"}"#;
        let raw = format!(
            "POST /api/open_repo HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let mut input = raw.into_bytes();
        input.extend_from_slice(body);
        let req = parse_request(Cursor::new(input)).expect("parse ok");
        assert_eq!(req.method, "POST");
        assert_eq!(req.target, "/api/open_repo");
        assert_eq!(req.body, body);
        assert_eq!(
            req.headers.get("content-length").map(String::as_str),
            Some(body.len().to_string().as_str())
        );
    }

    #[test]
    fn parse_request_truncated_request_is_malformed() {
        // EOF before the blank header-terminator line.
        let raw = b"GET /api/foo HTTP/1.1\r\nHost: localhost\r\n";
        let err = parse_request(Cursor::new(raw)).expect_err("must fail");
        match err {
            ParseError::Malformed => {}
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    #[test]
    fn parse_request_empty_request_line_is_malformed() {
        let raw = b"\r\n";
        let err = parse_request(Cursor::new(raw)).expect_err("must fail");
        match err {
            ParseError::Malformed => {}
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    #[test]
    fn parse_request_oversized_content_length_is_too_large() {
        let too_big = MAX_BODY_BYTES + 1;
        let raw = format!("POST /api/foo HTTP/1.1\r\nContent-Length: {too_big}\r\n\r\n");
        let err = parse_request(Cursor::new(raw.into_bytes())).expect_err("must fail");
        match err {
            ParseError::PayloadTooLarge => {}
            other => panic!("expected PayloadTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn parse_request_attacker_huge_content_length_does_not_allocate() {
        // Simulates the original DoS: client sends a 10 GB Content-Length but no body.
        // We must reject before allocating, not OOM.
        let raw = b"POST /api/foo HTTP/1.1\r\nContent-Length: 9999999999\r\n\r\n";
        let err = parse_request(Cursor::new(raw)).expect_err("must fail");
        assert!(matches!(err, ParseError::PayloadTooLarge));
    }

    #[test]
    fn parse_request_unparseable_content_length_treated_as_zero() {
        let raw = b"POST /api/foo HTTP/1.1\r\nContent-Length: not-a-number\r\n\r\n";
        let req = parse_request(Cursor::new(raw)).expect("parse ok");
        assert_eq!(req.method, "POST");
        assert!(req.body.is_empty());
    }

    #[test]
    fn parse_request_post_without_content_length_has_zero_body() {
        let raw = b"POST /api/foo HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = parse_request(Cursor::new(raw)).expect("parse ok");
        assert_eq!(req.method, "POST");
        assert!(req.body.is_empty());
    }

    #[test]
    fn parse_request_header_bytes_over_cap_is_too_large() {
        // Build a request whose headers alone exceed MAX_HEADER_BYTES.
        let big_value = "a".repeat(MAX_HEADER_BYTES + 1);
        let raw = format!("GET /api/foo HTTP/1.1\r\nX-Big: {big_value}\r\n\r\n");
        let err = parse_request(Cursor::new(raw.into_bytes())).expect_err("must fail");
        match err {
            ParseError::PayloadTooLarge => {}
            other => panic!("expected PayloadTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn parse_request_short_body_is_io_error() {
        // Content-Length says 10 but we only provide 3 bytes.
        let raw = b"POST /api/foo HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc";
        let err = parse_request(Cursor::new(raw)).expect_err("must fail");
        match err {
            ParseError::Io(_) => {}
            other => panic!("expected Io, got {other:?}"),
        }
    }

    // ----- split_target -----

    #[test]
    fn split_target_path_only() {
        let (path, query) = split_target("/api/foo");
        assert_eq!(path, "/api/foo");
        assert!(query.is_empty());
    }

    #[test]
    fn split_target_path_with_single_query() {
        let (path, query) = split_target("/api/foo?bar=baz");
        assert_eq!(path, "/api/foo");
        assert_eq!(query.get("bar").map(String::as_str), Some("baz"));
    }

    #[test]
    fn split_target_path_with_multiple_query_params() {
        let (path, query) = split_target("/api/x?a=1&b=2&c=3");
        assert_eq!(path, "/api/x");
        assert_eq!(query.get("a").map(String::as_str), Some("1"));
        assert_eq!(query.get("b").map(String::as_str), Some("2"));
        assert_eq!(query.get("c").map(String::as_str), Some("3"));
    }

    #[test]
    fn split_target_decodes_encoded_params() {
        let (path, query) = split_target("/api/x?path=%2Ftmp%2Ffoo+bar");
        assert_eq!(path, "/api/x");
        assert_eq!(query.get("path").map(String::as_str), Some("/tmp/foo bar"));
    }

    #[test]
    fn split_target_empty_query_segments_skipped() {
        let (_path, query) = split_target("/?&a=1&");
        // Empty pair fragments are filtered; only "a=1" remains.
        assert_eq!(query.len(), 1);
        assert_eq!(query.get("a").map(String::as_str), Some("1"));
    }

    // ----- percent_decode -----

    #[test]
    fn percent_decode_space_escape() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
    }

    #[test]
    fn percent_decode_plus_to_space() {
        assert_eq!(percent_decode("hello+world"), "hello world");
    }

    #[test]
    fn percent_decode_mixed_encodings() {
        assert_eq!(percent_decode("a+b%2Fc"), "a b/c");
    }

    #[test]
    fn percent_decode_invalid_escape_passes_through() {
        // "%ZZ" is not a valid hex escape; original bytes are preserved.
        assert_eq!(percent_decode("a%ZZb"), "a%ZZb");
    }

    #[test]
    fn percent_decode_trailing_percent_passes_through() {
        // A bare trailing '%' has no following hex digits.
        assert_eq!(percent_decode("foo%"), "foo%");
    }

    // ----- content_type -----

    #[test]
    fn content_type_known_extensions() {
        assert_eq!(
            content_type(Path::new("a.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("a.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("a.css")), "text/css; charset=utf-8");
        assert_eq!(content_type(Path::new("a.json")), "application/json");
        assert_eq!(content_type(Path::new("a.png")), "image/png");
        assert_eq!(content_type(Path::new("a.svg")), "image/svg+xml");
        assert_eq!(content_type(Path::new("a.pdf")), "application/pdf");
    }

    #[test]
    fn content_type_unknown_extension_is_octet_stream() {
        assert_eq!(content_type(Path::new("a.xyz")), "application/octet-stream");
        assert_eq!(content_type(Path::new("noext")), "application/octet-stream");
    }

    // ----- authorized -----

    fn header_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn authorized_bearer_match() {
        let h = header_map(&[("authorization", "Bearer secret-token")]);
        assert!(authorized(&h, "secret-token"));
    }

    #[test]
    fn authorized_bearer_mismatch() {
        let h = header_map(&[("authorization", "Bearer wrong-token")]);
        assert!(!authorized(&h, "secret-token"));
    }

    #[test]
    fn authorized_missing_authorization_header() {
        let h = header_map(&[("host", "localhost")]);
        assert!(!authorized(&h, "secret-token"));
    }

    #[test]
    fn authorized_non_bearer_scheme() {
        let h = header_map(&[("authorization", "Basic c2VjcmV0LXRva2Vu")]);
        assert!(!authorized(&h, "secret-token"));
    }

    #[test]
    fn authorized_bearer_prefix_only_no_token() {
        let h = header_map(&[("authorization", "Bearer ")]);
        assert!(!authorized(&h, "secret-token"));
    }
}
