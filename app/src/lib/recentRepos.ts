/// "Recent repositories" list shown on the welcome screen.
///
/// Persists the last N repos the user opened so re-opening one is a click
/// instead of a directory-picker round-trip. Pure localStorage, no
/// backend, no MCP — by design we forget across machines.

import { writable, type Writable } from 'svelte/store';

export interface RecentRepo {
  /// Absolute path to the repo root.
  path: string;
  /// Display name — usually `basename(path)`, but stored so we don't have
  /// to re-derive it.
  name: string;
  /// Cached counters from `RepoSummary` so the welcome cards show
  /// "12 modules · 87 classes" without reopening every entry.
  classes: number;
  modules: number;
  /// Wall-clock millis of the most recent open.
  openedAt: number;
}

const STORAGE_KEY = 'projectmind.recent_repos.v1';
const MAX = 10;

function load(): RecentRepo[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (e): e is RecentRepo =>
        e &&
        typeof e.path === 'string' &&
        typeof e.name === 'string' &&
        typeof e.openedAt === 'number',
    );
  } catch {
    return [];
  }
}

function save(list: RecentRepo[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(list));
  } catch {
    // ignore — storage unavailable / quota exceeded
  }
}

const inner: Writable<RecentRepo[]> = writable(load());

/// Subscribable store for the welcome screen.
export const recentRepos = { subscribe: inner.subscribe };

function basename(p: string): string {
  const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
  return idx === -1 ? p : p.slice(idx + 1);
}

/// Record a successful repo open. Move-to-front if the path is already in
/// the list, otherwise prepend; trim to `MAX`.
export function record(path: string, classes: number, modules: number) {
  inner.update((list) => {
    const filtered = list.filter((e) => e.path !== path);
    const next: RecentRepo[] = [
      { path, name: basename(path), classes, modules, openedAt: Date.now() },
      ...filtered,
    ].slice(0, MAX);
    save(next);
    return next;
  });
}

/// Remove a single entry. Used by the welcome card's `×` affordance.
export function forget(path: string) {
  inner.update((list) => {
    const next = list.filter((e) => e.path !== path);
    save(next);
    return next;
  });
}

/// Clear all entries. Wired to a future "Clear recent" menu item.
export function clear() {
  inner.set([]);
  save([]);
}
