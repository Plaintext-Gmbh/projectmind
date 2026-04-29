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
  | { kind: 'file'; path: string; anchor?: string | null };

export async function currentState(): Promise<UiState | null> {
  return invoke<UiState | null>('current_state');
}
