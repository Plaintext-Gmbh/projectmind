import { writable, derived } from 'svelte/store';
import type { ClassEntry, ModuleEntry, RepoSummary } from './api';

export const repo = writable<RepoSummary | null>(null);
export const modules = writable<ModuleEntry[]>([]);
export const classes = writable<ClassEntry[]>([]);
export const selectedClass = writable<ClassEntry | null>(null);
export const stereotypeFilter = writable<string | null>(null);
export const moduleFilter = writable<string | null>(null);
export const packageFilter = writable<string | null>(null);
export const errorMessage = writable<string | null>(null);
export type ViewMode =
  | 'classes'
  | 'diagram'
  | 'md'
  | 'file'
  | 'diff'
  | 'walkthrough'
  | 'html'
  | 'pdf'
  | 'image';

export interface WalkthroughCursor {
  id: string;
  step: number;
  /// Bumped on every applied intent so the view can re-fetch even when
  /// (id, step) is identical to last time (e.g. LLM rewrote step 0).
  nonce: number;
}
export const walkthroughCursor = writable<WalkthroughCursor | null>(null);
export const viewMode = writable<ViewMode>('classes');

export interface FileView {
  path: string;
  anchor: string | null;
  /// Bumped on every (re)issued intent, even if path/anchor stays the same.
  /// FileView listens to this to re-scroll on repeated MCP intents.
  nonce: number;
}
export const fileView = writable<FileView | null>(null);
export const diffViewRef = writable<{ reference: string; to: string | null } | null>(null);
/// When the GUI is currently following an MCP-driven view intent, this is true.
/// Used purely for UI affordances (tooltip / banner) — MCP always wins, so the
/// flag is informational, not a gate.
export const followingMcp = writable(false);

function packageOf(fqn: string): string {
  const idx = fqn.lastIndexOf('.');
  return idx === -1 ? '' : fqn.slice(0, idx);
}

function inPackage(fqn: string, pkg: string): boolean {
  if (pkg === '') return packageOf(fqn) === '';
  const own = packageOf(fqn);
  return own === pkg || own.startsWith(pkg + '.');
}

export const filteredClasses = derived(
  [classes, stereotypeFilter, moduleFilter, packageFilter],
  ([$classes, $stereo, $mod, $pkg]) =>
    $classes.filter(
      (c) =>
        ($stereo === null || c.stereotypes.includes($stereo)) &&
        ($mod === null || c.module === $mod) &&
        ($pkg === null || inPackage(c.fqn, $pkg)),
    ),
);

export const stereotypeCounts = derived(
  [classes, moduleFilter, packageFilter],
  ([$classes, $mod, $pkg]) => {
    const filtered = $classes.filter(
      (c) =>
        ($mod === null || c.module === $mod) && ($pkg === null || inPackage(c.fqn, $pkg)),
    );
    const counts: Record<string, number> = {};
    for (const c of filtered) {
      for (const s of c.stereotypes) {
        counts[s] = (counts[s] || 0) + 1;
      }
    }
    return counts;
  },
);
