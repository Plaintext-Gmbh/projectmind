import { invoke } from '@tauri-apps/api/core';

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
}

export interface ChangedFile {
  path: string;
  status: 'added' | 'modified' | 'deleted' | 'renamed' | 'type_change' | 'other';
}

export async function openRepo(path: string): Promise<RepoSummary> {
  return invoke<RepoSummary>('open_repo', { path });
}

export async function listClasses(stereotype?: string, module?: string): Promise<ClassEntry[]> {
  return invoke<ClassEntry[]>('list_classes', { stereotype, module });
}

export async function listModules(): Promise<ModuleEntry[]> {
  return invoke<ModuleEntry[]>('list_modules');
}

export async function showClass(
  fqn: string,
): Promise<{ source: string; file: string; line_start: number; line_end: number }> {
  return invoke('show_class', { fqn });
}

export async function listChangesSince(reference: string, to?: string): Promise<ChangedFile[]> {
  return invoke<ChangedFile[]>('list_changes_since', { reference, to });
}

export async function showDiagram(kind: 'bean-graph' | 'package-tree'): Promise<string> {
  return invoke<string>('show_diagram', { kind });
}

export async function showDiff(reference: string, to?: string): Promise<string> {
  return invoke<string>('show_diff', { reference, to });
}

export async function readFileText(path: string): Promise<string> {
  return invoke<string>('read_file_text', { path });
}

export interface MarkdownFile {
  abs: string;
  rel: string;
  title: string;
  size: number;
}

export async function listMarkdownFiles(root: string): Promise<MarkdownFile[]> {
  return invoke<MarkdownFile[]>('list_markdown_files', { root });
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
  return invoke<MarkdownHit[]>('search_markdown', { root, query, limit });
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
  return invoke<HtmlFile[]>('list_html_files', { root });
}

export async function findHtmlSnippets(root: string): Promise<HtmlSnippet[]> {
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
  return invoke<Walkthrough | null>('current_walkthrough');
}

export async function currentWalkthroughFeedback(): Promise<FeedbackLog> {
  return invoke<FeedbackLog>('current_walkthrough_feedback');
}

export async function ackWalkthrough(walkthroughId: string, step: number): Promise<FeedbackLog> {
  return invoke<FeedbackLog>('walkthrough_ack', { walkthroughId, step });
}

export async function requestMoreWalkthrough(
  walkthroughId: string,
  step: number,
  comment: string | null = null,
): Promise<FeedbackLog> {
  return invoke<FeedbackLog>('walkthrough_request_more', { walkthroughId, step, comment });
}

export async function setWalkthroughStep(id: string, step: number): Promise<void> {
  return invoke<void>('set_walkthrough_step', { id, step });
}

export async function endWalkthrough(): Promise<void> {
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
  return invoke<UiState | null>('current_state');
}
