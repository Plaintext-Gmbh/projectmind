import { describe, expect, it } from 'vitest';
import { buildDiffIndex, focusLineIndex, parseHunkHeader, type DiffLine } from './diffFocus';

const SAMPLE: DiffLine[] = [
  { kind: 'header', text: 'diff --git a/src/a.ts b/src/a.ts' },
  { kind: 'meta', text: 'index 1234..5678 100644' },
  { kind: 'meta', text: '--- a/src/a.ts' },
  { kind: 'meta', text: '+++ b/src/a.ts' },
  { kind: 'hunk', text: '@@ -10,3 +10,4 @@' },
  { kind: 'context', text: ' ten' },
  { kind: 'add', text: '+eleven' },
  { kind: 'context', text: ' twelve' },
  { kind: 'context', text: ' thirteen' },
  { kind: 'hunk', text: '@@ -100,2 +101,3 @@' },
  { kind: 'context', text: ' hundred-one' },
  { kind: 'add', text: '+hundred-two' },
  { kind: 'context', text: ' hundred-three' },
  { kind: 'header', text: 'diff --git a/src/b.ts b/src/b.ts' },
  { kind: 'meta', text: '--- a/src/b.ts' },
  { kind: 'meta', text: '+++ b/src/b.ts' },
  { kind: 'hunk', text: '@@ -1,2 +1,3 @@' },
  { kind: 'context', text: ' one' },
  { kind: 'add', text: '+two' },
  { kind: 'context', text: ' three' },
];

describe('parseHunkHeader', () => {
  it('extracts old + new starts from canonical headers', () => {
    expect(parseHunkHeader('@@ -10,3 +10,4 @@ extra')).toEqual({ oldStart: 10, newStart: 10 });
  });

  it('handles single-line hunks without a count', () => {
    expect(parseHunkHeader('@@ -5 +5 @@')).toEqual({ oldStart: 5, newStart: 5 });
  });

  it('returns null for malformed headers', () => {
    expect(parseHunkHeader('@@ broken')).toBeNull();
  });
});

describe('focusLineIndex', () => {
  it('returns null when no focus is requested', () => {
    expect(focusLineIndex(SAMPLE, undefined)).toBeNull();
    expect(focusLineIndex(SAMPLE, {})).toBeNull();
  });

  it('jumps to the first hunk of a named file when `hunk` and `line` are omitted', () => {
    expect(focusLineIndex(SAMPLE, { file: 'b.ts' })).toBe(16);
  });

  it('jumps to a specific hunk index inside a file', () => {
    expect(focusLineIndex(SAMPLE, { file: 'a.ts', hunk: 1 })).toBe(9);
  });

  it('clamps an out-of-range hunk index to the first hunk', () => {
    expect(focusLineIndex(SAMPLE, { file: 'a.ts', hunk: 99 })).toBe(4);
  });

  it('walks forward to the first add/context line matching `line`', () => {
    // file=a.ts, second hunk starts new-side at 101.
    // Lines: 101=hundred-one (context), 102=hundred-two (add), 103=hundred-three (context).
    expect(focusLineIndex(SAMPLE, { file: 'a.ts', hunk: 1, line: 102 })).toBe(11);
  });

  it('falls back to the hunk anchor when `line` does not match any line in the hunk', () => {
    expect(focusLineIndex(SAMPLE, { file: 'a.ts', hunk: 1, line: 9999 })).toBe(9);
  });

  it('returns -1 sentinel (null) when the requested file is not in the diff', () => {
    expect(focusLineIndex(SAMPLE, { file: 'c.ts' })).toBeNull();
  });

  it('handles `line` without a file by searching the first hunk', () => {
    // Without a file, the first hunk (@@ -10,3 +10,4 @@) starts new-side at 10.
    // Line 11 is the +eleven add line at index 6.
    expect(focusLineIndex(SAMPLE, { line: 11 })).toBe(6);
  });
});

describe('buildDiffIndex', () => {
  it('groups hunks under their files with new-side paths and line anchors', () => {
    const idx = buildDiffIndex(SAMPLE);
    expect(idx.map((f) => f.newPath)).toEqual(['src/a.ts', 'src/b.ts']);
    // a.ts: two hunks starting at flat indices 4 and 9.
    expect(idx[0].hunks.map((h) => h.startLine)).toEqual([4, 9]);
    expect(idx[0].hunks.map((h) => h.index)).toEqual([0, 1]);
    expect(idx[0].hunks.map((h) => h.newStart)).toEqual([10, 101]);
    // b.ts: one hunk starting at flat index 16.
    expect(idx[1].hunks.map((h) => h.startLine)).toEqual([16]);
    expect(idx[1].startLine).toBe(13);
  });

  it('counts adds and dels per hunk', () => {
    const idx = buildDiffIndex(SAMPLE);
    // Each hunk in SAMPLE has exactly one add and no del.
    expect(idx[0].hunks.map((h) => [h.adds, h.dels])).toEqual([
      [1, 0],
      [1, 0],
    ]);
    expect(idx[1].hunks[0].adds).toBe(1);
    expect(idx[1].hunks[0].dels).toBe(0);
  });

  it('mixes adds and dels correctly', () => {
    const lines: DiffLine[] = [
      { kind: 'header', text: 'diff --git a/x.ts b/x.ts' },
      { kind: 'meta', text: '--- a/x.ts' },
      { kind: 'meta', text: '+++ b/x.ts' },
      { kind: 'hunk', text: '@@ -1,3 +1,3 @@' },
      { kind: 'del', text: '-old' },
      { kind: 'add', text: '+new' },
      { kind: 'context', text: ' keep' },
    ];
    const idx = buildDiffIndex(lines);
    expect(idx).toHaveLength(1);
    expect(idx[0].hunks[0].adds).toBe(1);
    expect(idx[0].hunks[0].dels).toBe(1);
  });

  it('uses the old-side path for pure deletions (`+++ /dev/null`)', () => {
    const lines: DiffLine[] = [
      { kind: 'header', text: 'diff --git a/gone.ts b/gone.ts' },
      { kind: 'meta', text: 'deleted file mode 100644' },
      { kind: 'meta', text: '--- a/gone.ts' },
      { kind: 'meta', text: '+++ /dev/null' },
      { kind: 'hunk', text: '@@ -1,2 +0,0 @@' },
      { kind: 'del', text: '-a' },
      { kind: 'del', text: '-b' },
    ];
    const idx = buildDiffIndex(lines);
    expect(idx[0].newPath).toBe('gone.ts');
    expect(idx[0].hunks[0].dels).toBe(2);
  });

  it('returns an empty index for an empty diff', () => {
    expect(buildDiffIndex([])).toEqual([]);
  });

  it('homes a bare hunk fragment without a file header', () => {
    const lines: DiffLine[] = [
      { kind: 'hunk', text: '@@ -1 +1 @@' },
      { kind: 'add', text: '+x' },
    ];
    const idx = buildDiffIndex(lines);
    expect(idx).toHaveLength(1);
    expect(idx[0].newPath).toBe('(diff)');
    expect(idx[0].hunks).toHaveLength(1);
  });
});
