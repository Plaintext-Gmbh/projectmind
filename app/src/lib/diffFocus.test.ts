import { describe, expect, it } from 'vitest';
import { focusLineIndex, parseHunkHeader, type DiffLine } from './diffFocus';

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
