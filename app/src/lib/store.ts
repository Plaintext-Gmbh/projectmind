import { writable, derived, get } from 'svelte/store';
import type { ClassEntry, DiagramKind, ModuleEntry, ModuleFile, RepoSummary } from './api';

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
export const diagramKind = writable<DiagramKind>('bean-graph');

export interface NavState {
  viewMode: ViewMode;
  diagramKind: DiagramKind;
  fileView: FileView | null;
  diffViewRef: { reference: string; to: string | null } | null;
  selectedFqn: string | null;
}

const MAX_NAV_HISTORY = 80;
export const navBackStack = writable<NavState[]>([]);
export const navForwardStack = writable<NavState[]>([]);
export const canGoBack = derived(navBackStack, ($stack) => $stack.length > 0);
export const canGoForward = derived(navForwardStack, ($stack) => $stack.length > 0);

export function currentNavState(): NavState {
  return {
    viewMode: get(viewMode),
    diagramKind: get(diagramKind),
    fileView: cloneFileView(get(fileView)),
    diffViewRef: cloneDiffRef(get(diffViewRef)),
    selectedFqn: get(selectedClass)?.fqn ?? null,
  };
}

export function navigateTo(target: Partial<NavState>) {
  const current = currentNavState();
  const next: NavState = {
    ...current,
    ...target,
    fileView: target.fileView !== undefined ? cloneFileView(target.fileView) : current.fileView,
    diffViewRef:
      target.diffViewRef !== undefined ? cloneDiffRef(target.diffViewRef) : current.diffViewRef,
  };
  if (sameNavState(current, next)) return;
  navBackStack.update((stack) => [...stack.slice(-(MAX_NAV_HISTORY - 1)), current]);
  navForwardStack.set([]);
  applyNavState(next);
}

export function replaceNavState(target: Partial<NavState>) {
  const current = currentNavState();
  applyNavState({
    ...current,
    ...target,
    fileView: target.fileView !== undefined ? cloneFileView(target.fileView) : current.fileView,
    diffViewRef:
      target.diffViewRef !== undefined ? cloneDiffRef(target.diffViewRef) : current.diffViewRef,
  });
}

export function goBack() {
  const stack = get(navBackStack);
  const previous = stack[stack.length - 1];
  if (!previous) return;
  navBackStack.set(stack.slice(0, -1));
  navForwardStack.update((forward) => [currentNavState(), ...forward].slice(0, MAX_NAV_HISTORY));
  applyNavState(previous);
}

export function goForward() {
  const stack = get(navForwardStack);
  const next = stack[0];
  if (!next) return;
  navForwardStack.set(stack.slice(1));
  navBackStack.update((back) => [...back.slice(-(MAX_NAV_HISTORY - 1)), currentNavState()]);
  applyNavState(next);
}

export function clearNavigationHistory() {
  navBackStack.set([]);
  navForwardStack.set([]);
}

function applyNavState(state: NavState) {
  diagramKind.set(state.diagramKind);
  fileView.set(cloneFileView(state.fileView));
  diffViewRef.set(cloneDiffRef(state.diffViewRef));
  if (state.selectedFqn) {
    selectedClass.set(get(classes).find((c) => c.fqn === state.selectedFqn) ?? null);
  } else {
    selectedClass.set(null);
  }
  viewMode.set(state.viewMode);
}

function cloneFileView(value: FileView | null): FileView | null {
  return value ? { ...value } : null;
}

function cloneDiffRef(
  value: { reference: string; to: string | null } | null,
): { reference: string; to: string | null } | null {
  return value ? { ...value } : null;
}

function sameNavState(a: NavState, b: NavState): boolean {
  return (
    a.viewMode === b.viewMode &&
    a.diagramKind === b.diagramKind &&
    a.selectedFqn === b.selectedFqn &&
    JSON.stringify(a.fileView) === JSON.stringify(b.fileView) &&
    JSON.stringify(a.diffViewRef) === JSON.stringify(b.diffViewRef)
  );
}

/// Visibility flags for the two left-hand panes in code view. Persisted in
/// localStorage so the user's layout choice survives reloads.
function persistedBool(key: string, fallback: boolean) {
  let initial = fallback;
  try {
    const raw = localStorage.getItem(key);
    if (raw !== null) initial = raw === 'true';
  } catch {
    // localStorage unavailable
  }
  const store = writable<boolean>(initial);
  store.subscribe((v) => {
    try {
      localStorage.setItem(key, String(v));
    } catch {
      // ignore
    }
  });
  return store;
}

export const moduleSidebarVisible = persistedBool('projectmind.layout.modules.visible', true);
export const classSidebarVisible = persistedBool('projectmind.layout.files.visible', true);

const RECENT_REPOS_KEY = 'projectmind.recentRepos';
const RECENT_REPOS_MAX = 10;

function readRecentRepos(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_REPOS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) {
      return parsed.filter((x): x is string => typeof x === 'string');
    }
  } catch {
    // ignore
  }
  return [];
}

export const recentRepos = writable<string[]>(readRecentRepos());

recentRepos.subscribe((value) => {
  try {
    localStorage.setItem(RECENT_REPOS_KEY, JSON.stringify(value));
  } catch {
    // ignore
  }
});

/// Move (or insert) a repo path to the top of the MRU list, capped at
/// RECENT_REPOS_MAX. Called from the App after a successful openRepo.
export function rememberRepo(path: string) {
  if (!path) return;
  recentRepos.update((cur) => {
    const next = [path, ...cur.filter((p) => p !== path)];
    return next.slice(0, RECENT_REPOS_MAX);
  });
}

export function forgetRecentRepo(path: string) {
  recentRepos.update((cur) => cur.filter((p) => p !== path));
}

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

/// Returns true when an absolute file path lives inside the given module's
/// root directory. Used by MarkdownIndex/HtmlIndex to honour the global
/// moduleFilter when listing files.
export function fileBelongsToModule(abs: string, mod: ModuleEntry): boolean {
  if (!mod.root) return false;
  return abs === mod.root || abs.startsWith(mod.root.endsWith('/') ? mod.root : mod.root + '/');
}

/// Resolves the module that owns the given absolute file path, if any. Picks
/// the longest-prefix match so nested modules win over their parents.
export function moduleForFile(abs: string, mods: ModuleEntry[]): ModuleEntry | null {
  let best: ModuleEntry | null = null;
  for (const m of mods) {
    if (fileBelongsToModule(abs, m) && (best === null || m.root.length > best.root.length)) {
      best = m;
    }
  }
  return best;
}
