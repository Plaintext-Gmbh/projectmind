import { writable, derived } from 'svelte/store';
import type { ClassEntry, ModuleEntry, ModuleFile, RepoSummary } from './api';

export const repo = writable<RepoSummary | null>(null);
export const modules = writable<ModuleEntry[]>([]);
export const classes = writable<ClassEntry[]>([]);
/// Module-files (PDFs, images, …) keyed by module id. App.svelte populates
/// this map whenever moduleFilter / modules change. Lifted into the store so
/// the modules-sidebar can derive per-module file counts for its badges, and
/// the right-pane list can present files alongside classes.
export const moduleFilesByModule = writable<Record<string, ModuleFile[]>>({});
export const selectedClass = writable<ClassEntry | null>(null);
export const stereotypeFilter = writable<string | null>(null);
/// Mutually exclusive with stereotypeFilter — selects only module files
/// of the given kind (e.g. 'pdf', 'png'). Setting one clears the other.
export const fileKindFilter = writable<string | null>(null);
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

/// Files visible under the current moduleFilter — used by the right-pane
/// mixed list. When no module is filtered we fan out across every module.
export const filteredModuleFiles = derived(
  [moduleFilesByModule, moduleFilter],
  ([$byMod, $mod]) => {
    if ($mod !== null) return $byMod[$mod] ?? [];
    return Object.values($byMod).flat();
  },
);

/// Module → number of non-source files. Used by ModuleSidebar to display a
/// file count when a module has 0 parsed classes.
export const fileCountByModule = derived(moduleFilesByModule, ($byMod) => {
  const counts: Record<string, number> = {};
  for (const [id, files] of Object.entries($byMod)) {
    counts[id] = files.length;
  }
  return counts;
});
