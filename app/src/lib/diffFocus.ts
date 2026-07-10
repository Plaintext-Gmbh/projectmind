/// Diff focus helpers for the tour focus rail (#126).
///
/// Pure functions: take a parsed unified-diff line stream and a
/// `DiffFocus` request, return the line index to scroll to. Lifted out
/// of `DiffView.svelte` so vitest can pin the math without booting the
/// component.

import type { DiffFocus } from './api';

/// Tagged unified-diff line — same shape `DiffView.svelte` produces.
export interface DiffLine {
  kind: 'meta' | 'header' | 'add' | 'del' | 'context' | 'hunk';
  text: string;
}

/// Find the 0-based index in `lines` that the focus request points at.
/// Returns `null` when nothing matches — caller leaves the diff at its
/// natural starting position.
///
/// Resolution order:
///   1. Restrict the search to `focus.file`'s file-block when set.
///      Match is substring on the `+++ b/<path>` line so callers can
///      pass full paths or basenames.
///   2. Within that scope, pick the `hunk`-th hunk header (0-based).
///   3. If `line` is set, walk forward from that hunk and find the
///      first `add` or `context` line whose new-side line number
///      equals `line`. (Hunk header `@@ -a,b +c,d @@` → start at `c`.)
export function focusLineIndex(
  lines: readonly DiffLine[],
  focus: DiffFocus | undefined,
): number | null {
  if (!focus || (!focus.file && focus.hunk === undefined && focus.line === undefined)) {
    return null;
  }

  const { fileStart, fileEnd } = fileWindow(lines, focus.file);
  if (fileStart === -1) return null;

  const hunkStarts: number[] = [];
  for (let i = fileStart; i < fileEnd; i++) {
    if (lines[i].kind === 'hunk') hunkStarts.push(i);
  }

  // No hunks at all → bail out gracefully.
  if (hunkStarts.length === 0) return fileStart < lines.length ? fileStart : null;

  const hunkIdx =
    focus.hunk !== undefined && focus.hunk >= 0 && focus.hunk < hunkStarts.length
      ? focus.hunk
      : 0;
  const hunkAt = hunkStarts[hunkIdx];

  if (focus.line === undefined) return hunkAt;

  // Walk forward through the hunk body looking for a +/context line whose
  // new-side line number equals `focus.line`.
  const newStart = parseHunkHeader(lines[hunkAt].text)?.newStart ?? null;
  if (newStart === null) return hunkAt;

  let cur = newStart;
  for (let i = hunkAt + 1; i < fileEnd; i++) {
    const l = lines[i];
    if (l.kind === 'hunk') break; // next hunk — out of scope
    if (l.kind === 'header' || l.kind === 'meta') continue;
    if (l.kind === 'add' || l.kind === 'context') {
      if (cur === focus.line) return i;
      cur += 1;
    }
    // `del` lines don't advance the new-side counter.
  }
  return hunkAt;
}

/// Find `[start, end)` of the diff block that contains `file`, or
/// `[0, lines.length)` when no file is requested. Returns `-1` for the
/// start when the file isn't in the diff.
export function fileWindow(
  lines: readonly DiffLine[],
  file: string | undefined,
): { fileStart: number; fileEnd: number } {
  if (!file) return { fileStart: 0, fileEnd: lines.length };
  const blocks = fileBlocks(lines);
  for (const b of blocks) {
    // The header lives a few lines after `diff --git` — match on any
    // header / meta line that mentions the requested path.
    for (let i = b.start; i < b.end; i++) {
      const l = lines[i];
      if ((l.kind === 'header' || l.kind === 'meta') && l.text.includes(file)) {
        return { fileStart: b.start, fileEnd: b.end };
      }
    }
  }
  return { fileStart: -1, fileEnd: -1 };
}

/// Split the diff into per-file blocks delimited by `diff --git` headers.
function fileBlocks(lines: readonly DiffLine[]): { start: number; end: number }[] {
  const out: { start: number; end: number }[] = [];
  let cur = -1;
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].kind === 'header') {
      if (cur !== -1) out.push({ start: cur, end: i });
      cur = i;
    }
  }
  if (cur !== -1) out.push({ start: cur, end: lines.length });
  // Diffs without any file header (e.g. when only `+`/`-` lines were
  // captured) still need a single window so the focus logic doesn't
  // bail out.
  if (out.length === 0 && lines.length > 0) out.push({ start: 0, end: lines.length });
  return out;
}

/// Parse a `@@ -a,b +c,d @@` hunk header. Returns the new-side line
/// numbers (1-based start). `null` for malformed headers.
export function parseHunkHeader(
  header: string,
): { oldStart: number; newStart: number } | null {
  // `@@ -a,b +c,d @@ optional-context`
  const m = /^@@\s+-(\d+)(?:,\d+)?\s+\+(\d+)(?:,\d+)?\s+@@/.exec(header);
  if (!m) return null;
  return { oldStart: Number(m[1]), newStart: Number(m[2]) };
}

/// One navigable hunk inside a `DiffFile`. `startLine` indexes the flat
/// `lines[]` array so the rail can `scrollIntoView` the matching element;
/// `adds`/`dels` drive the marker's magnitude glyph.
export interface DiffHunk {
  /// 0-based hunk index within its file (mirrors `DiffFocus.hunk`).
  index: number;
  /// The raw `@@ … @@` header text.
  header: string;
  /// Index into the flat `lines[]` array of the `hunk` line.
  startLine: number;
  /// New-side 1-based start line, or `null` for a malformed header.
  newStart: number | null;
  /// Count of `+` lines in the hunk body.
  adds: number;
  /// Count of `-` lines in the hunk body.
  dels: number;
}

/// One file block in the diff, with its navigable hunks. `newPath` is the
/// display path (basename shown in the rail); `startLine` points at the
/// `diff --git` / first line of the block for file-level jumps.
export interface DiffFile {
  /// Display path — the `+++ b/<path>` target, falling back to the
  /// `--- a/<path>` source for deletions, else the `diff --git` label.
  newPath: string;
  /// Index into the flat `lines[]` array of the file block's first line.
  startLine: number;
  hunks: DiffHunk[];
}

/// Strip a `--- a/` / `+++ b/` prefix (and the surrounding decorations)
/// off a meta line, returning the bare path or `null` when the line has
/// no usable path.
function metaPath(text: string): string | null {
  const m = /^(?:---|\+\+\+)\s+(?:[ab]\/)?(.+?)\s*$/.exec(text);
  if (!m) return null;
  const p = m[1].trim();
  return p === '/dev/null' || p === '' ? null : p;
}

/// Build the structured per-file / per-hunk navigation index from the
/// flat diff-line stream (#126). Purely derived from `lines`; the flat
/// render stays authoritative for the visual diff, this is the parallel
/// structure the `<DiffRail>` navigates.
export function buildDiffIndex(lines: readonly DiffLine[]): DiffFile[] {
  const files: DiffFile[] = [];
  let file: DiffFile | null = null;
  let hunk: DiffHunk | null = null;

  const closeHunk = () => {
    if (hunk && file) file.hunks.push(hunk);
    hunk = null;
  };

  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    if (l.kind === 'header') {
      closeHunk();
      // Derive an initial display path from the `diff --git a/x b/y` line;
      // the `+++ b/…` meta line below refines it.
      const gm = /^diff --git a\/(.+?) b\/(.+)$/.exec(l.text);
      file = { newPath: gm ? gm[2] : l.text.replace(/^diff --git\s+/, ''), startLine: i, hunks: [] };
      files.push(file);
      continue;
    }
    if (l.kind === 'meta' && file) {
      // Prefer the new-side path; fall back to the old-side for deletions.
      if (l.text.startsWith('+++ ')) {
        const p = metaPath(l.text);
        if (p) file.newPath = p;
      } else if (l.text.startsWith('--- ') && !file.hunks.length) {
        const p = metaPath(l.text);
        if (p && (file.newPath === '/dev/null' || file.newPath.startsWith('diff --git'))) {
          file.newPath = p;
        }
      }
      continue;
    }
    if (l.kind === 'hunk') {
      closeHunk();
      // A hunk with no preceding `diff --git` header (raw fragment) still
      // needs a home so the rail can address it.
      if (!file) {
        file = { newPath: '(diff)', startLine: i, hunks: [] };
        files.push(file);
      }
      hunk = {
        index: file.hunks.length,
        header: l.text,
        startLine: i,
        newStart: parseHunkHeader(l.text)?.newStart ?? null,
        adds: 0,
        dels: 0,
      };
      continue;
    }
    if (hunk) {
      if (l.kind === 'add') hunk.adds += 1;
      else if (l.kind === 'del') hunk.dels += 1;
    }
  }
  closeHunk();
  return files;
}
