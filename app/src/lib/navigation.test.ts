/// Unit tests for the navigation history layer.
///
/// Each test isolates the module graph (`vi.resetModules()`) so the
/// in-module `inner` writable starts fresh, and wipes localStorage so
/// persisted state from one test doesn't bleed into the next.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

type Nav = typeof import('./navigation');

const STORAGE_KEY = 'projectmind.history.v1';

function entry(label: string, overrides: Partial<import('./navigation').HistoryEntry> = {}) {
  // Minimum-viable entry — every other field nulled — so de-dup tests
  // exercise the comparator on a single dimension at a time.
  return {
    viewMode: 'classes',
    selectedFqn: null,
    filePath: null,
    fileAnchor: null,
    diagramKind: null,
    folderMapLayout: null,
    diffRef: null,
    diffTo: null,
    walkthroughId: null,
    walkthroughStep: null,
    moduleFilter: null,
    packageFilter: null,
    stereotypeFilter: null,
    fileKindFilter: null,
    label,
    ts: 0,
    ...overrides,
  } as import('./navigation').HistoryEntry;
}

async function freshNav(): Promise<Nav> {
  vi.resetModules();
  return await import('./navigation');
}

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  localStorage.clear();
});

describe('push', () => {
  it('records the first entry and moves the cursor to it', async () => {
    const nav = await freshNav();
    nav.push(entry('a', { selectedFqn: 'A' }));
    const h = get(nav.history);
    expect(h.entries).toHaveLength(1);
    expect(h.cursor).toBe(0);
    expect(h.entries[0].selectedFqn).toBe('A');
  });

  it('de-dupes consecutive identical entries', async () => {
    const nav = await freshNav();
    nav.push(entry('a', { selectedFqn: 'A' }));
    nav.push(entry('a-again', { selectedFqn: 'A' })); // same nav state, different label
    const h = get(nav.history);
    expect(h.entries).toHaveLength(1);
  });

  it('does NOT de-dupe when navigation state actually changes', async () => {
    const nav = await freshNav();
    nav.push(entry('a', { selectedFqn: 'A' }));
    nav.push(entry('b', { selectedFqn: 'B' }));
    const h = get(nav.history);
    expect(h.entries.map((e) => e.selectedFqn)).toEqual(['A', 'B']);
  });

  it('truncates forward history when pushing after a back step', async () => {
    const nav = await freshNav();
    nav.push(entry('a', { selectedFqn: 'A' }));
    nav.push(entry('b', { selectedFqn: 'B' }));
    nav.push(entry('c', { selectedFqn: 'C' }));
    nav.back(() => {}); // cursor → 1
    nav.push(entry('d', { selectedFqn: 'D' })); // discards C
    const h = get(nav.history);
    expect(h.entries.map((e) => e.selectedFqn)).toEqual(['A', 'B', 'D']);
    expect(h.cursor).toBe(2);
  });
});

describe('back / forward', () => {
  it('back applies the previous entry and updates the cursor', async () => {
    const nav = await freshNav();
    nav.push(entry('a', { selectedFqn: 'A' }));
    nav.push(entry('b', { selectedFqn: 'B' }));
    let applied: string | null = null;
    expect(nav.back((e) => { applied = e.selectedFqn; })).toBe(true);
    expect(applied).toBe('A');
    expect(get(nav.history).cursor).toBe(0);
  });

  it('back returns false when already at the first entry', async () => {
    const nav = await freshNav();
    nav.push(entry('a'));
    expect(nav.back(() => {})).toBe(false);
  });

  it('forward returns false when already at the last entry', async () => {
    const nav = await freshNav();
    nav.push(entry('a'));
    nav.push(entry('b'));
    expect(nav.forward(() => {})).toBe(false);
  });

  it('forward replays a previously-popped entry', async () => {
    const nav = await freshNav();
    nav.push(entry('a', { selectedFqn: 'A' }));
    nav.push(entry('b', { selectedFqn: 'B' }));
    nav.back(() => {});
    let applied: string | null = null;
    expect(nav.forward((e) => { applied = e.selectedFqn; })).toBe(true);
    expect(applied).toBe('B');
    expect(get(nav.history).cursor).toBe(1);
  });

  it('push during a back/forward apply does not pollute history', async () => {
    const nav = await freshNav();
    nav.push(entry('a', { selectedFqn: 'A' }));
    nav.push(entry('b', { selectedFqn: 'B' }));
    // The user-facing apply callback often triggers store changes that
    // would normally call push themselves; the `applying` flag should
    // silence those during back().
    nav.back(() => {
      nav.push(entry('side-effect', { selectedFqn: 'X' }));
    });
    const h = get(nav.history);
    expect(h.entries.map((e) => e.selectedFqn)).toEqual(['A', 'B']);
  });
});

describe('canBack / canForward', () => {
  it('canBack tracks cursor > 0', async () => {
    const nav = await freshNav();
    expect(get(nav.canBack)).toBe(false);
    nav.push(entry('a', { selectedFqn: 'A' }));
    expect(get(nav.canBack)).toBe(false);
    nav.push(entry('b', { selectedFqn: 'B' }));
    expect(get(nav.canBack)).toBe(true);
    nav.back(() => {});
    expect(get(nav.canBack)).toBe(false);
  });

  it('canForward tracks cursor < entries.length - 1', async () => {
    const nav = await freshNav();
    expect(get(nav.canForward)).toBe(false);
    nav.push(entry('a', { selectedFqn: 'A' }));
    nav.push(entry('b', { selectedFqn: 'B' }));
    expect(get(nav.canForward)).toBe(false);
    nav.back(() => {});
    expect(get(nav.canForward)).toBe(true);
  });
});

describe('persistence', () => {
  it('saves to localStorage on push', async () => {
    const nav = await freshNav();
    nav.push(entry('a', { selectedFqn: 'A' }));
    const raw = localStorage.getItem(STORAGE_KEY);
    expect(raw).not.toBeNull();
    const parsed = JSON.parse(raw as string);
    expect(parsed.entries).toHaveLength(1);
    expect(parsed.cursor).toBe(0);
  });

  it('restores from localStorage on module load', async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      entries: [entry('persisted', { selectedFqn: 'P' })],
      cursor: 0,
    }));
    const nav = await freshNav();
    const h = get(nav.history);
    expect(h.entries).toHaveLength(1);
    expect(h.entries[0].selectedFqn).toBe('P');
  });

  it('clamps an out-of-bounds cursor restored from storage', async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      entries: [entry('a')],
      cursor: 99,
    }));
    const nav = await freshNav();
    expect(get(nav.history).cursor).toBe(0);
  });

  it('falls back to empty when storage holds garbage', async () => {
    localStorage.setItem(STORAGE_KEY, '{not json');
    const nav = await freshNav();
    const h = get(nav.history);
    expect(h.entries).toEqual([]);
    expect(h.cursor).toBe(-1);
  });
});

describe('clear', () => {
  it('wipes both in-memory and persisted state', async () => {
    const nav = await freshNav();
    nav.push(entry('a'));
    nav.push(entry('b'));
    nav.clear();
    expect(get(nav.history).entries).toEqual([]);
    expect(get(nav.history).cursor).toBe(-1);
    const raw = JSON.parse(localStorage.getItem(STORAGE_KEY) as string);
    expect(raw.entries).toEqual([]);
  });
});
