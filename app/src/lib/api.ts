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
