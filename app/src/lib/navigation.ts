/// Browser-style back/forward navigation history.
///
/// Snapshots the user-visible navigation state (viewMode, selected
/// class, open file, diagram kind, filters) and stacks them up so
/// ←/→ buttons + keyboard shortcuts can step the user through their
/// own footprints exactly the way a browser does.
///
/// Deliberately tiny on its own: the wiring lives in App.svelte where
/// the relevant stores are already imported. This module only owns
/// the data structure, the de-dup rule, the persistence, and the
/// forward-truncation semantics.

import { derived, get, writable, type Writable } from 'svelte/store';

export type DiagramKind =
  | 'bean-graph'
  | 'package-tree'
  | 'folder-map'
  | 'inheritance-tree'
  | 'doc-graph'
  | 'c4-container'
  | 'architecture-layers';
export type FolderMapLayout = 'hierarchy' | 'solar' | 'td';

/// Frozen snapshot of every navigation-relevant piece of state.
/// Plain JSON values only — no live store references — so it can
/// round-trip through localStorage without surprises.
export interface HistoryEntry {
  viewMode: string;
  selectedFqn: string | null;
  filePath: string | null;
  fileAnchor: string | null;
  diagramKind: DiagramKind | null;
  folderMapLayout: FolderMapLayout | null;
  diffRef: string | null;
  diffTo: string | null;
  walkthroughId: string | null;
  walkthroughStep: number | null;
  moduleFilter: string | null;
  packageFilter: string | null;
  stereotypeFilter: string | null;
  fileKindFilter: string | null;
  /// Short human-readable hint (e.g. "Class · UserService", "File ·
  /// docs/SYNC.md") for the back-button title attribute and the
  /// optional history dropdown.
  label: string;
  /// Wall-clock time of the push, for the dropdown's relative-age
  /// rendering ("2 min ago").
  ts: number;
}

interface HistoryState {
  entries: HistoryEntry[];
  cursor: number;
}

const STORAGE_KEY = 'projectmind.history.v1';
const MAX_ENTRIES = 200;

function loadFromStorage(): HistoryState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { entries: [], cursor: -1 };
    const parsed = JSON.parse(raw) as Partial<HistoryState>;
    if (!Array.isArray(parsed.entries) || typeof parsed.cursor !== 'number') {
      return { entries: [], cursor: -1 };
    }
    const cursor = Math.max(-1, Math.min(parsed.entries.length - 1, parsed.cursor));
    return { entries: parsed.entries as HistoryEntry[], cursor };
  } catch {
    return { entries: [], cursor: -1 };
  }
}

function saveToStorage(state: HistoryState) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // localStorage unavailable / quota exceeded — silently ignore;
    // history just won't survive reloads.
  }
}

/// True iff two entries describe the same user-visible state. Used
/// to drop redundant pushes ("user clicked the same class twice").
function entriesEqual(a: HistoryEntry, b: HistoryEntry): boolean {
  return (
    a.viewMode === b.viewMode &&
    a.selectedFqn === b.selectedFqn &&
    a.filePath === b.filePath &&
    a.fileAnchor === b.fileAnchor &&
    a.diagramKind === b.diagramKind &&
    a.folderMapLayout === b.folderMapLayout &&
    a.diffRef === b.diffRef &&
    a.diffTo === b.diffTo &&
    a.walkthroughId === b.walkthroughId &&
    a.walkthroughStep === b.walkthroughStep &&
    a.moduleFilter === b.moduleFilter &&
    a.packageFilter === b.packageFilter &&
    a.stereotypeFilter === b.stereotypeFilter &&
    a.fileKindFilter === b.fileKindFilter
  );
}

const inner: Writable<HistoryState> = writable(loadFromStorage());

/// Read-only view of the history state. Use `canBack` / `canForward`
/// for button-enable logic and `entries` / `cursor` for the optional
/// history dropdown.
export const history = { subscribe: inner.subscribe };

export const canBack = derived(inner, ($h) => $h.cursor > 0);
export const canForward = derived(inner, ($h) => $h.cursor < $h.entries.length - 1);

/// Set to `true` while we're applying a back/forward navigation, so
/// the corresponding store-change subscribers in App.svelte know not
/// to push the resulting state as a new entry. Exposed to the outer
/// world as a function to keep the implementation detail tight.
let applying = false;
export function isApplyingHistory(): boolean {
  return applying;
}

/// Push a new entry onto the history stack. De-dupes against the
/// current entry so re-clicks don't fill the back button with copies
/// of the same state. Truncates forward history (standard browser
/// behaviour: navigating after a Back drops the redo trail).
export function push(entry: HistoryEntry) {
  if (applying) return;
  inner.update((h) => {
    const current = h.cursor >= 0 ? h.entries[h.cursor] : null;
    if (current && entriesEqual(current, entry)) return h;
    const head = h.entries.slice(0, h.cursor + 1);
    head.push(entry);
    // Cap the trail so localStorage doesn't grow without bound.
    const trimmed = head.length > MAX_ENTRIES ? head.slice(-MAX_ENTRIES) : head;
    const next: HistoryState = { entries: trimmed, cursor: trimmed.length - 1 };
    saveToStorage(next);
    return next;
  });
}

/// Step one entry backwards and call `apply` with the entry that
/// becomes current. The caller is responsible for translating an
/// entry back into store writes.
export function back(apply: (entry: HistoryEntry) => void): boolean {
  const h = get(inner);
  if (h.cursor <= 0) return false;
  const cursor = h.cursor - 1;
  applying = true;
  try {
    apply(h.entries[cursor]);
  } finally {
    applying = false;
  }
  const next = { ...h, cursor };
  inner.set(next);
  saveToStorage(next);
  return true;
}

/// Step one entry forwards. Counterpart to `back`.
export function forward(apply: (entry: HistoryEntry) => void): boolean {
  const h = get(inner);
  if (h.cursor >= h.entries.length - 1) return false;
  const cursor = h.cursor + 1;
  applying = true;
  try {
    apply(h.entries[cursor]);
  } finally {
    applying = false;
  }
  const next = { ...h, cursor };
  inner.set(next);
  saveToStorage(next);
  return true;
}

/// Wipe the history. Wired to the future "Clear history" menu entry;
/// also useful from the dev console.
export function clear() {
  const next: HistoryState = { entries: [], cursor: -1 };
  inner.set(next);
  saveToStorage(next);
}
