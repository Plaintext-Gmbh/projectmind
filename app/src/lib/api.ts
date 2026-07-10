import { convertFileSrc, invoke } from '@tauri-apps/api/core';

const TOKEN_KEY = 'projectmind.browser.token';

export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export function browserToken(): string | null {
  if (typeof window === 'undefined') return null;
  const fromHash = new URLSearchParams(window.location.hash.replace(/^#/, '')).get('token');
  const fromQuery = new URLSearchParams(window.location.search).get('token');
  const token = fromHash || fromQuery;
  if (token) {
    localStorage.setItem(TOKEN_KEY, token);
    if (fromQuery && !fromHash) {
      window.history.replaceState(null, '', `${window.location.pathname}#token=${token}`);
    }
    return token;
  }
  return localStorage.getItem(TOKEN_KEY);
}

export function setBrowserToken(token: string): void {
  localStorage.setItem(TOKEN_KEY, token.trim());
}

export function clearBrowserToken(): void {
  localStorage.removeItem(TOKEN_KEY);
}

function query(params: Record<string, string | number | null | undefined>): string {
  const q = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== null && value !== undefined && value !== '') q.set(key, String(value));
  }
  const s = q.toString();
  return s ? `?${s}` : '';
}

async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const token = browserToken();
  if (!token) throw new Error('Browser token required');
  const headers = new Headers(init.headers);
  headers.set('Authorization', `Bearer ${token}`);
  if (init.body && !headers.has('Content-Type')) headers.set('Content-Type', 'application/json');
  const res = await fetch(path, { ...init, headers });
  if (!res.ok) {
    let msg = `${res.status} ${res.statusText}`;
    try {
      const body = await res.json();
      if (body?.error) msg = body.error;
    } catch {
      // keep HTTP status
    }
    throw new Error(msg);
  }
  return res.json() as Promise<T>;
}

function post<T>(path: string, body: unknown): Promise<T> {
  return api<T>(path, { method: 'POST', body: JSON.stringify(body) });
}

export interface ClassEntry {
  fqn: string;
  name: string;
  file: string;
  stereotypes: string[];
  kind: string;
  module: string;
}

export interface ModuleEntry {
  id: string;
  name: string;
  root: string;
  classes: number;
  stereotypes: Record<string, number>;
}

export interface TabDescriptor {
  /// Stable id, used as Svelte each-key (e.g. "files", "diagrams", "tests").
  id: string;
  /// i18n key for the visible label.
  label_key: string;
  /// Frontend viewMode the tab activates.
  view_mode: string;
}

export interface RepoSummary {
  root: string;
  modules: number;
  classes: number;
  language_plugins: string[];
  framework_plugins: string[];
  markdown_count: number;
  html_count: number;
  /// Diagram kinds available for this repo + plugin set (e.g. "bean-graph",
  /// "package-tree", "folder-map"). Returned by the backend so the UI can
  /// render the Diagram-tab buttons dynamically.
  available_diagrams: string[];
  /// Top-level UI tabs the active plugin set contributes. Core ships
  /// "files" + "diagrams"; plugins can append more (e.g. a future
  /// "framework-junit" "Tests" tab). Rendered in declaration order.
  tabs: TabDescriptor[];
  /// Code-graph cache backend in effect for this repo ("memory" |
  /// "sqlite"); null when no cache is configured. Diagnostics for the
  /// persistence selection in .projectmind/config.toml.
  code_graph_backend: string | null;
}

export interface ChangedFile {
  path: string;
  status: 'added' | 'modified' | 'deleted' | 'renamed' | 'type_change' | 'other';
}

export async function openRepo(path: string): Promise<RepoSummary> {
  // A new repo invalidates the memoised Risk Atlas backing badge annotations.
  invalidateRiskCache();
  if (!isTauriRuntime()) return post<RepoSummary>('/api/open_repo', { path });
  return invoke<RepoSummary>('open_repo', { path });
}

export async function openMarkdownFile(path: string): Promise<RepoSummary> {
  if (!isTauriRuntime()) return post<RepoSummary>('/api/open_markdown_file', { path });
  return invoke<RepoSummary>('open_markdown_file', { path });
}

export async function pendingMarkdownFile(): Promise<string | null> {
  if (!isTauriRuntime()) return null;
  return invoke<string | null>('pending_markdown_file');
}

export async function listClasses(stereotype?: string, module?: string): Promise<ClassEntry[]> {
  if (!isTauriRuntime()) {
    return api<ClassEntry[]>(`/api/list_classes${query({ stereotype, module })}`);
  }
  return invoke<ClassEntry[]>('list_classes', { stereotype, module });
}

export async function listModules(): Promise<ModuleEntry[]> {
  if (!isTauriRuntime()) return api<ModuleEntry[]>('/api/list_modules');
  return invoke<ModuleEntry[]>('list_modules');
}

export async function showClass(
  fqn: string,
): Promise<{ source: string; file: string; line_start: number; line_end: number }> {
  if (!isTauriRuntime()) return api(`/api/show_class${query({ fqn })}`);
  return invoke('show_class', { fqn });
}

export interface AnnotationRef {
  /// Simple name without the leading `@`.
  name: string;
  /// Raw arguments inside the parentheses (e.g. `value="/users", method=GET`),
  /// or null for plain marker annotations like `@Override`.
  raw_args: string | null;
}

export interface MethodOutline {
  name: string;
  visibility: 'public' | 'protected' | 'package' | 'private';
  is_static: boolean;
  line_start: number;
  line_end: number;
  annotations: AnnotationRef[];
}

export interface FieldOutline {
  name: string;
  type: string;
  visibility: 'public' | 'protected' | 'package' | 'private';
  is_static: boolean;
  line: number;
  annotations: AnnotationRef[];
}

export interface SuperTypeOutline {
  /// Type name as written in source ("AbstractEntity", "Display", "java.io.Serializable").
  name: string;
  /// "extends" or "implements" (Rust trait-impls land as "implements" too).
  kind: 'extends' | 'implements';
}

/// Per-class coverage resolved from an ingested report (Cockpit 2.2, Issue #158).
/// Omitted entirely when no coverage data maps onto the class.
export interface ClassCoverage {
  /// Line coverage fraction, 0.0..=1.0.
  line: number;
  /// Source report format ("jacoco" | "lcov" | "cobertura").
  format: string;
  /// True when the report's mtime is older than 24h.
  stale: boolean;
  /// Report age in seconds, or null when unknown.
  age_secs: number | null;
}

export interface ClassOutline {
  fqn: string;
  name: string;
  kind: string;
  visibility: 'public' | 'protected' | 'package' | 'private';
  line_start: number;
  line_end: number;
  stereotypes: string[];
  annotations: AnnotationRef[];
  methods: MethodOutline[];
  fields: FieldOutline[];
  /// Declared parent types in source order — drives the inheritance crumb
  /// rendered above the class name.
  super_types: SuperTypeOutline[];
  /// Coverage for this class, when a report resolves to it. Absent otherwise.
  coverage?: ClassCoverage;
}

export async function classOutline(fqn: string): Promise<ClassOutline> {
  if (!isTauriRuntime()) return api<ClassOutline>(`/api/class_outline${query({ fqn })}`);
  return invoke<ClassOutline>('class_outline', { fqn });
}

export async function listChangesSince(reference: string, to?: string): Promise<ChangedFile[]> {
  if (!isTauriRuntime()) {
    return api<ChangedFile[]>(`/api/list_changes_since${query({ reference, to })}`);
  }
  return invoke<ChangedFile[]>('list_changes_since', { reference, to });
}

export type GitRefKind = 'branch' | 'tag';

export interface GitRef {
  /// Short name as the user types it ("master", "feature/foo", "v1.0.0").
  name: string;
  /// Branch vs tag.
  kind: GitRefKind;
  /// 7-char target commit SHA. Empty if the target could not be resolved.
  target_sha: string;
}

/// List local branches + tags in the open repository. Branches come first
/// (`master`/`main` floated to the top), then tags sorted descending by name.
export async function listRefs(): Promise<GitRef[]> {
  if (!isTauriRuntime()) return api<GitRef[]>('/api/list_refs');
  return invoke<GitRef[]>('list_refs');
}

export interface FileRecency {
  /// Repository-relative path.
  path: string;
  /// Seconds since UNIX epoch when the most recent touching commit was authored.
  last_commit_secs: number;
  /// Seconds elapsed between that commit and the time `file_recency` ran.
  secs_ago: number;
  /// Short (7-char) commit hash of the most recent touching commit.
  sha: string;
  /// First line of that commit's message.
  summary: string;
  /// Author display name of the most-recent touching commit, or null when
  /// the signature was missing or empty.
  author_name: string | null;
  /// Author email of the same commit. Combined with `author_name` it gives
  /// the stable identity the GUI's author overlay hashes onto a hue.
  author_email: string | null;
}

/// Per-file recency index for the open repo. Drives change-map visualisations
/// (heatmap by recency, author overlay, diff overlay, timeline river — see #63).
/// Sorted newest-first; capped at 5,000 entries.
export async function fileRecency(): Promise<FileRecency[]> {
  if (!isTauriRuntime()) return api<FileRecency[]>('/api/file_recency');
  return invoke<FileRecency[]>('file_recency');
}

/// One persisted user annotation. Mirrors `AnnotationRecord` on the Rust
/// side; lines are 1-based and inclusive on both ends.
export interface AnnotationRecord {
  id: number;
  /// Repository-relative path, forward-slash separators.
  file: string;
  line_from: number;
  line_to: number;
  /// Short label (e.g. ticket id, "TODO: simplify", reviewer note).
  label: string;
  /// Optional external link the user wants the marker to jump to.
  link: string | null;
  /// Free-form metadata reserved for future plugins / integrations.
  extras: Record<string, unknown>;
}

/// Payload for adding a new annotation. The store assigns the id; any
/// caller-supplied id is ignored.
export interface AnnotationInput {
  file: string;
  line_from: number;
  line_to: number;
  label: string;
  link?: string | null;
}

/// Fetch annotations for the open repo. Pass a repo-relative `file` to
/// scope the response to one file; omit it for every annotation in the
/// repo.
export async function listAnnotations(file?: string): Promise<AnnotationRecord[]> {
  if (!isTauriRuntime()) {
    return api<AnnotationRecord[]>(`/api/list_annotations${query({ file })}`);
  }
  return invoke<AnnotationRecord[]>('list_annotations', { file });
}

/// Add an annotation. Returns the id the store allocated.
export async function addAnnotation(annotation: AnnotationInput): Promise<number> {
  if (!isTauriRuntime()) {
    const result = await post<{ id: number }>('/api/add_annotation', annotation);
    return result.id;
  }
  return invoke<number>('add_annotation', { annotation });
}

/// Remove an annotation by id. Idempotent: removing an unknown id succeeds
/// silently.
export async function removeAnnotation(id: number): Promise<void> {
  if (!isTauriRuntime()) {
    await post<{ ok: boolean }>('/api/remove_annotation', { id });
    return;
  }
  await invoke('remove_annotation', { id });
}

export type DiagramKind =
  | 'bean-graph'
  | 'package-tree'
  | 'folder-map'
  | 'inheritance-tree'
  | 'doc-graph'
  | 'c4-container'
  | 'architecture-layers'
  | 'architecture-flow'
  | 'module-chord'
  | 'activity-heatmap'
  | 'language-stats';

export async function showDiagram(kind: DiagramKind): Promise<string> {
  if (!isTauriRuntime()) return api<string>(`/api/show_diagram${query({ kind })}`);
  return invoke<string>('show_diagram', { kind });
}

export async function showDiff(reference: string, to?: string): Promise<string> {
  if (!isTauriRuntime()) return api<string>(`/api/show_diff${query({ reference, to })}`);
  return invoke<string>('show_diff', { reference, to });
}

export async function readFileText(path: string): Promise<string> {
  if (!isTauriRuntime()) return api<string>(`/api/read_file_text${query({ path })}`);
  return invoke<string>('read_file_text', { path });
}

/**
 * Reveal a file or folder in the OS file manager (Finder / Explorer / Files).
 * Desktop-only; in browser mode this is a no-op since the host file manager
 * isn't reachable from a remote browser session.
 */
export async function revealInFileManager(path: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke('reveal_in_file_manager', { path });
}

export async function fileAssetUrl(path: string): Promise<string> {
  if (isTauriRuntime()) return convertFileSrc(path);
  const token = browserToken();
  if (!token) throw new Error('Browser token required');
  const res = await fetch(`/api/read_file_bytes${query({ path })}`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!res.ok) {
    let msg = `${res.status} ${res.statusText}`;
    try {
      const body = await res.json();
      if (body?.error) msg = body.error;
    } catch {
      // keep HTTP status
    }
    throw new Error(msg);
  }
  return URL.createObjectURL(await res.blob());
}

export interface MarkdownFile {
  abs: string;
  rel: string;
  title: string;
  size: number;
}

export async function listMarkdownFiles(root: string): Promise<MarkdownFile[]> {
  if (!isTauriRuntime()) return api<MarkdownFile[]>(`/api/list_markdown_files${query({ root })}`);
  return invoke<MarkdownFile[]>('list_markdown_files', { root });
}

export interface ModuleFile {
  abs: string;
  rel: string;
  kind: string;
  size: number;
}

export async function listModuleFiles(moduleId: string): Promise<ModuleFile[]> {
  if (!isTauriRuntime()) {
    return api<ModuleFile[]>(`/api/list_module_files${query({ module: moduleId })}`);
  }
  return invoke<ModuleFile[]>('list_module_files', { moduleId });
}

export type MatchKind = 'title' | 'path' | 'content';

export interface MarkdownHit {
  file: MarkdownFile;
  score: number;
  matched_in: MatchKind;
  snippet: string | null;
}

export async function searchMarkdown(
  root: string,
  query: string,
  limit = 200,
): Promise<MarkdownHit[]> {
  if (!isTauriRuntime()) {
    return api<MarkdownHit[]>(`/api/search_markdown${windowQuery({ root, query, limit })}`);
  }
  return invoke<MarkdownHit[]>('search_markdown', { root, query, limit });
}

function windowQuery(params: Record<string, string | number | null | undefined>): string {
  return query(params);
}

export type HtmlKind = 'html' | 'xhtml' | 'jsp' | 'velocity' | 'freemarker';

export interface HtmlFile {
  abs: string;
  rel: string;
  kind: HtmlKind;
  size: number;
}

export interface HtmlSnippet {
  abs: string;
  rel: string;
  line: number;
  lang: string;
  content: string;
  tag_count: number;
}

export async function listHtmlFiles(root: string): Promise<HtmlFile[]> {
  if (!isTauriRuntime()) return api<HtmlFile[]>(`/api/list_html_files${query({ root })}`);
  return invoke<HtmlFile[]>('list_html_files', { root });
}

export async function findHtmlSnippets(root: string): Promise<HtmlSnippet[]> {
  if (!isTauriRuntime()) return api<HtmlSnippet[]>(`/api/find_html_snippets${query({ root })}`);
  return invoke<HtmlSnippet[]>('find_html_snippets', { root });
}

// ----- Walk-through --------------------------------------------------------

export interface LineRange {
  from: number;
  to: number;
}

/// Optional spotlight inside a diff target (#126).
export interface DiffFocus {
  /// Repo-relative path or basename. Substring match on the
  /// `+++ b/<path>` header.
  file?: string;
  /// 0-based hunk index inside `file` (or the whole diff when no file).
  hunk?: number;
  /// 1-based line number in the new file.
  line?: number;
}

/** Risk signals a `risk` step's header bar can surface (Cockpit 2.4, #160). */
export type RiskSignal = 'churn' | 'cx' | 'cov' | 'fan_in' | 'fan_out';

export type WalkthroughTarget =
  | { kind: 'class'; fqn: string; highlight?: LineRange[] }
  | { kind: 'file'; path: string; anchor?: string | null; highlight?: LineRange[] }
  | { kind: 'diff'; reference: string; to?: string | null; focus?: DiffFocus }
  | { kind: 'artifact'; id: string }
  // Cockpit 2.4 (#160): risk-scored class with a header bar.
  | { kind: 'risk'; fqn: string; focus?: string | null; show?: RiskSignal[] }
  // Cockpit 2.4 (#160): one architecture-drift pattern's violation list.
  | { kind: 'pattern'; pattern: string; scope?: string | null; view?: string | null }
  // Cockpit 2.4 (#160): Risk Atlas treemap with named hotspots ringed.
  | { kind: 'atlas'; module?: string | null; highlight_fqns?: string[] }
  | { kind: 'note' };

export interface WalkthroughStep {
  title: string;
  narration?: string;
  target: WalkthroughTarget;
}

export interface QuizQuestion {
  prompt: string;
  choices: string[];
  /// 0-based index into `choices` of the correct answer.
  answer: number;
  /// 0-based step indices that explain this question. The GUI shows
  /// them as "replay these steps" links when the user gets the
  /// question wrong.
  step_refs?: number[];
  /// Plain-text explanation shown after the user answers. Not markdown.
  explanation?: string;
}

export interface Walkthrough {
  /// On-disk schema version (Cockpit 2.4, #160). Missing on v1 tours
  /// authored before the risk/pattern/atlas kinds existed; the loader
  /// treats absence as v1 so old tours render unchanged.
  schemaVersion?: number;
  id: string;
  title: string;
  summary?: string;
  steps: WalkthroughStep[];
  /// Optional end-of-tour quiz. Empty / missing when the tour author
  /// didn't include one — the GUI then keeps the existing "Tour
  /// finished" card without any quiz UI.
  quiz?: QuizQuestion[];
  updated_at: number;
}

export type FeedbackKind = 'understood' | 'more_detail';

export interface FeedbackEvent {
  walkthrough_id: string;
  step: number;
  kind: FeedbackKind;
  comment?: string | null;
  ts: number;
}

export interface FeedbackLog {
  events: FeedbackEvent[];
}

export async function currentWalkthrough(): Promise<Walkthrough | null> {
  if (!isTauriRuntime()) return api<Walkthrough | null>('/api/current_walkthrough');
  return invoke<Walkthrough | null>('current_walkthrough');
}

export async function currentWalkthroughFeedback(): Promise<FeedbackLog> {
  if (!isTauriRuntime()) return api<FeedbackLog>('/api/current_walkthrough_feedback');
  return invoke<FeedbackLog>('current_walkthrough_feedback');
}

export async function ackWalkthrough(walkthroughId: string, step: number): Promise<FeedbackLog> {
  if (!isTauriRuntime()) {
    return post<FeedbackLog>('/api/walkthrough_ack', { walkthrough_id: walkthroughId, step });
  }
  return invoke<FeedbackLog>('walkthrough_ack', { walkthroughId, step });
}

export async function requestMoreWalkthrough(
  walkthroughId: string,
  step: number,
  comment: string | null = null,
): Promise<FeedbackLog> {
  if (!isTauriRuntime()) {
    return post<FeedbackLog>('/api/walkthrough_request_more', {
      walkthrough_id: walkthroughId,
      step,
      comment,
    });
  }
  return invoke<FeedbackLog>('walkthrough_request_more', { walkthroughId, step, comment });
}

export async function setWalkthroughStep(id: string, step: number): Promise<void> {
  if (!isTauriRuntime()) return post<void>('/api/set_walkthrough_step', { id, step });
  return invoke<void>('set_walkthrough_step', { id, step });
}

export async function endWalkthrough(): Promise<void> {
  if (!isTauriRuntime()) return post<void>('/api/end_walkthrough', {});
  return invoke<void>('end_walkthrough');
}

// ----- Artifacts -----------------------------------------------------------

export type ArtifactFormat = 'html' | 'markdown';

/// A full AI-generated artifact body pushed via `present_artifact`.
export interface Artifact {
  id: string;
  title: string;
  format: ArtifactFormat;
  /// HTML source (rendered in a sandboxed iframe) or Markdown.
  content: string;
  /// Content byte length.
  size: number;
  created_at: number;
  updated_at: number;
}

/// Lightweight artifact metadata for `list_artifacts` — no body.
export interface ArtifactMeta {
  id: string;
  title: string;
  format: ArtifactFormat;
  size: number;
  created_at: number;
  updated_at: number;
}

/// Fetch a single artifact body by id, or null when it no longer exists.
export async function currentArtifact(id: string): Promise<Artifact | null> {
  if (!isTauriRuntime()) return api<Artifact | null>(`/api/current_artifact${query({ id })}`);
  return invoke<Artifact | null>('current_artifact', { id });
}

/// List metadata for every stored artifact.
export async function listArtifacts(): Promise<ArtifactMeta[]> {
  if (!isTauriRuntime()) return api<ArtifactMeta[]>('/api/list_artifacts');
  return invoke<ArtifactMeta[]>('list_artifacts');
}

export interface UiState {
  version: number;
  repo_root: string | null;
  view: ViewIntent;
  seq: number;
}

export type ViewIntent =
  | { kind: 'classes'; selected_fqn?: string | null }
  | { kind: 'diagram'; diagram_kind: string }
  | { kind: 'diff'; reference: string; to?: string | null }
  | { kind: 'file'; path: string; anchor?: string | null }
  | { kind: 'walkthrough'; id: string; step: number }
  | { kind: 'artifact'; id: string };

export async function currentState(): Promise<UiState | null> {
  if (!isTauriRuntime()) return api<UiState | null>('/api/current_state');
  return invoke<UiState | null>('current_state');
}

/// Build-integrity markers — surfaced via the shield button in the header.
/// Lets the user verify whether the desktop app they are running was produced
/// by the official tagged-release pipeline (signed bundle, embedded official
/// updater public key) or is a self-compiled / forked build.
export interface BuildIntegrity {
  version: string;
  is_release_build: boolean;
  git_commit: string | null;
  built_at: string | null;
  updater_pubkey_hash: string;
  updater_pubkey_short: string;
}

export async function getBuildIntegrity(): Promise<BuildIntegrity | null> {
  if (!isTauriRuntime()) return null;
  return invoke<BuildIntegrity>('get_build_integrity');
}

// ---- Risk Atlas (Cockpit 2.1, Issue #157) -------------------------------

export interface RiskScore {
  fqn: string;
  module: string;
  file: string;
  /** Composite score 0..=100. */
  score: number;
  churn: number;
  cx: number;
  sloc: number;
  /** Line coverage fraction 0..=1, or null when no report resolves. */
  cov: number | null;
  /** Number of classes that depend on this class (incoming edges). */
  fan_in: number;
  /** Number of classes this class depends on (outgoing edges). */
  fan_out: number;
  why: string;
}

export interface RiskWeights {
  churn: number;
  cx: number;
  cov: number;
  deps: number;
}

/// Metadata for the coverage report backing a Risk Atlas run
/// (Cockpit 2.2, Issue #158). Absent when no report was ingested.
export interface CoverageMeta {
  format: string;
  path: string;
  age_secs: number | null;
  stale: boolean;
}

export interface RiskAtlasResult {
  window_days: number;
  weights: RiskWeights;
  scores: RiskScore[];
  /// The coverage report used for the `cov` scores, when one was resolved.
  coverage?: CoverageMeta;
}

export interface RiskAtlasOptions {
  module?: string;
  top?: number;
  windowDays?: number;
  weights?: Partial<RiskWeights>;
}

/**
 * Risk Atlas: per-class churn+complexity score for the treemap. Dual-mode
 * (Tauri invoke / browser-host HTTP), wie alle anderen Reads.
 */
export async function riskAtlas(opts: RiskAtlasOptions = {}): Promise<RiskAtlasResult> {
  const { module, top, windowDays, weights } = opts;
  if (!isTauriRuntime()) {
    return api<RiskAtlasResult>(
      `/api/risk_atlas${query({
        module,
        top,
        window_days: windowDays,
        churn: weights?.churn,
        cx: weights?.cx,
        cov: weights?.cov,
        deps: weights?.deps,
      })}`,
    );
  }
  return invoke<RiskAtlasResult>('risk_atlas', {
    module,
    top,
    windowDays,
    weights,
  });
}

// ---- Auto-annotation atlas cache (Cockpit 2.4, #160) --------------------
//
// Walkthrough class/risk steps need per-fqn risk signals to draw badges.
// Fetching the atlas per step would be wasteful (git churn walk + coverage
// scan), so the full atlas is fetched once and memoised as a fqn→score map.
// The promise itself is cached so concurrent step opens share one request.
// `invalidateRiskCache()` drops it when a new repo is opened.

let riskScoreMapCache: Promise<Map<string, RiskScore>> | null = null;

/**
 * Fetch the whole Risk Atlas once and index it by fqn, memoising the result
 * for the lifetime of the open repo. Cheap on repeat calls; a single git +
 * coverage pass on first use. `top` is large so every class resolves.
 */
export async function riskScoreMap(): Promise<Map<string, RiskScore>> {
  if (!riskScoreMapCache) {
    riskScoreMapCache = riskAtlas({ top: 5000 })
      .then((res) => {
        const map = new Map<string, RiskScore>();
        for (const s of res.scores) map.set(s.fqn, s);
        return map;
      })
      .catch((err) => {
        // Degrade gracefully: a missing/failed atlas means "no badges",
        // never a broken tour. Drop the cache so a later retry can succeed.
        riskScoreMapCache = null;
        throw err;
      });
  }
  return riskScoreMapCache;
}

/** Look up the risk score for one fqn, or `null` when the atlas has no row. */
export async function riskScoreFor(fqn: string): Promise<RiskScore | null> {
  try {
    const map = await riskScoreMap();
    return map.get(fqn) ?? null;
  } catch {
    return null;
  }
}

/** Drop the memoised atlas — called when a different repo is opened. */
export function invalidateRiskCache(): void {
  riskScoreMapCache = null;
}

// ---- Pattern Lens (Cockpit 2.3, Issue #159) -----------------------------

/** One module's compliance count for a pattern: classes that satisfy the rule. */
export interface ModuleHold {
  module: string;
  count: number;
}

/** One concrete architecture-drift violation. */
export interface PatternViolation {
  module: string;
  file: string;
  /** 1-based line; 0 when the offending element has no line attached. */
  line: number;
  fqn: string;
  message: string;
  /** 1=info, 2=warn, 3=critical. */
  severity: number;
  /** How clearly this hit matches the rule (0..=1). Below 0.6 = noise. */
  confidence: number;
}

/** Aggregate result for one detector. */
export interface PatternResult {
  /** snake_case pattern id: repository | layered | di_only | tx_on_service | no_static_state. */
  pattern: string;
  holds: ModuleHold[];
  violations: PatternViolation[];
  /** Detector-level confidence 0..=1. */
  confidence: number;
}

/** Violations below this confidence are hidden from the heatmap (noise floor). */
export const PATTERN_CONFIDENCE_FLOOR = 0.6;

/** Human-readable label per snake_case pattern id (heatmap rows). */
export const PATTERN_LABELS: Record<string, string> = {
  repository: 'Repository',
  layered: 'Layered',
  di_only: 'DI-only',
  tx_on_service: '@Tx boundary',
  no_static_state: 'No static state',
};

export interface PatternCheckOptions {
  /** snake_case or PascalCase pattern id; omit for the full heatmap. */
  pattern?: string;
  /** Module id filter; omit for the whole repo. */
  module?: string;
}

/**
 * Pattern Lens: run architecture-drift detectors and return one
 * {@link PatternResult} per active pattern (the compliance heatmap). Omitting
 * `pattern` runs every enabled detector; passing it narrows to one. Dual-mode
 * (Tauri invoke / browser-host HTTP), like every other read.
 */
export async function patternCheck(opts: PatternCheckOptions = {}): Promise<PatternResult[]> {
  const { pattern, module } = opts;
  if (!isTauriRuntime()) {
    return api<PatternResult[]>(`/api/pattern_check${query({ pattern, module })}`);
  }
  return invoke<PatternResult[]>('pattern_check', { pattern, module });
}

/** One step in a {@link WalkthroughQueryResult} (Cockpit 2.5, #161). */
export interface WalkthroughQueryStep {
  title: string;
  fqn?: string;
  file?: string;
  /** 1-based inclusive `[from, to]` line span, when known. */
  lines?: [number, number];
  narration: string;
  /** Similarity of this step to the question, 0..1. */
  score: number;
}

/** Answer of a semantic tour lookup ({@link walkthroughQuery}). */
export interface WalkthroughQueryResult {
  /** Winning tour id, or absent when nothing matched. */
  tour_id?: string;
  steps: WalkthroughQueryStep[];
  /** Max similarity found, 0..1. */
  confidence: number;
  /**
   * `"grep"` when no tour answered well (weak match, nothing indexed, or the
   * server was built without the `embed` feature) — the caller should fall
   * back to normal search. Absent when the answer is good.
   */
  fallback?: 'grep';
}

export interface WalkthroughQueryOptions {
  /** Tour ids to bias the match toward (a nudge, not an override). */
  preferTours?: string[];
  /** Max steps to return for the best tour. */
  topK?: number;
}

/**
 * Semantic tour lookup (Cockpit 2.5, #161): match a natural-language
 * question against the curated walk-through tours and return the best tour's
 * steps. Dual-mode (Tauri invoke / browser-host HTTP), like every other read.
 * Without the `embed` feature the backend answers `fallback: "grep"`.
 */
export async function walkthroughQuery(
  question: string,
  opts: WalkthroughQueryOptions = {},
): Promise<WalkthroughQueryResult> {
  const { preferTours, topK } = opts;
  if (!isTauriRuntime()) {
    return api<WalkthroughQueryResult>(
      `/api/walkthrough_query${query({
        question,
        prefer_tours: preferTours?.join(','),
        top_k: topK,
      })}`,
    );
  }
  return invoke<WalkthroughQueryResult>('walkthrough_query', {
    question,
    preferTours,
    topK,
  });
}
