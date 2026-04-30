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
}

export interface ChangedFile {
  path: string;
  status: 'added' | 'modified' | 'deleted' | 'renamed' | 'type_change' | 'other';
}

export async function openRepo(path: string): Promise<RepoSummary> {
  if (!isTauriRuntime()) return post<RepoSummary>('/api/open_repo', { path });
  return invoke<RepoSummary>('open_repo', { path });
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

export async function listChangesSince(reference: string, to?: string): Promise<ChangedFile[]> {
  if (!isTauriRuntime()) {
    return api<ChangedFile[]>(`/api/list_changes_since${query({ reference, to })}`);
  }
  return invoke<ChangedFile[]>('list_changes_since', { reference, to });
}

export type DiagramKind = 'bean-graph' | 'package-tree' | 'folder-map';

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

export type WalkthroughTarget =
  | { kind: 'class'; fqn: string; highlight?: LineRange[] }
  | { kind: 'file'; path: string; anchor?: string | null; highlight?: LineRange[] }
  | { kind: 'diff'; reference: string; to?: string | null }
  | { kind: 'note' };

export interface WalkthroughStep {
  title: string;
  narration?: string;
  target: WalkthroughTarget;
}

export interface Walkthrough {
  id: string;
  title: string;
  summary?: string;
  steps: WalkthroughStep[];
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
  | { kind: 'walkthrough'; id: string; step: number };

export async function currentState(): Promise<UiState | null> {
  if (!isTauriRuntime()) return api<UiState | null>('/api/current_state');
  return invoke<UiState | null>('current_state');
}
