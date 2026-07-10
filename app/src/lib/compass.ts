/// Change-compass orientation helpers (#127).
///
/// The walkthrough viewer renders a single-line strip below each step's
/// target hint that shows *where this step sits in the codebase*. The
/// helpers here keep the logic out of the Svelte component so vitest can
/// pin the breadcrumb math without booting a DOM.
import type { ChangedFile, WalkthroughStep } from './api';

/// Breadcrumb segments for a tour step. Empty for `note` targets — those
/// are stage-direction cards without a code anchor.
///
/// - **class**: last three FQN segments (`com.foo.bar.baz.UserSvc` →
///   `bar`, `baz`, `UserSvc`). Falls back to the full FQN when the
///   class lives in a top-level namespace.
/// - **file**: last four path segments. Repos with deep nesting (Maven
///   `src/main/java/...`) would otherwise dominate the strip.
/// - **diff**: a single segment with the ref range (`HEAD~5..HEAD` or
///   `HEAD~5 → working tree`).
/// - **risk**: like class — last three FQN segments (Cockpit 2.4).
/// - **pattern**: the pattern id, plus its scope when set (Cockpit 2.4).
/// - **atlas**: a single `atlas · <module|repo>` segment (Cockpit 2.4).
/// - **note** / unknown: no crumbs.
export function compassFor(t: WalkthroughStep['target'] | undefined): string[] {
  if (!t) return [];
  if (t.kind === 'class' && t.fqn) {
    const parts = t.fqn.split('.').filter(Boolean);
    const tail = parts.slice(-3);
    return tail.length > 1 ? tail : parts;
  }
  if (t.kind === 'file' && t.path) {
    const parts = t.path.split(/[\\/]/).filter(Boolean);
    return parts.length > 4 ? parts.slice(-4) : parts;
  }
  if (t.kind === 'diff') {
    const range = t.to ? `${t.reference}..${t.to}` : `${t.reference} → working tree`;
    return [range];
  }
  // Cockpit 2.4 kinds (#160).
  if (t.kind === 'risk' && t.fqn) {
    const parts = t.fqn.split('.').filter(Boolean);
    const tail = parts.slice(-3);
    return tail.length > 1 ? tail : parts;
  }
  if (t.kind === 'pattern') {
    return t.scope ? [t.pattern, t.scope] : [t.pattern];
  }
  if (t.kind === 'atlas') {
    return [t.module ? `atlas · ${t.module}` : 'atlas · repo'];
  }
  // #125: before/after diagram snapshot — crumb is the ref range.
  if (t.kind === 'diagram-diff') {
    const range = t.to ? `${t.from}..${t.to}` : `${t.from} → working tree`;
    return [range];
  }
  return [];
}

/// Normalise a path to forward slashes with no leading `./` so paths from
/// different sources (git deltas are repo-relative, tour targets may be
/// absolute) compare on their tails.
function normPath(p: string): string {
  return p.replace(/\\/g, '/').replace(/^\.\//, '');
}

/// True when `changedPath` (repo-relative, from `list_changes_since`)
/// refers to the same file as `targetPath` (which may be absolute).
/// Suffix match on a path-segment boundary so `a/b/User.ts` matches
/// `/repo/a/b/User.ts` but not `Other.ts`.
function samePath(targetPath: string, changedPath: string): boolean {
  const a = normPath(targetPath);
  const b = normPath(changedPath);
  if (a === b) return true;
  if (a.endsWith('/' + b)) return true;
  if (b.endsWith('/' + a)) return true;
  return false;
}

/// Whether the step's target file appears in the tour's changed-file set.
///
/// - `changed` — the target path is in `changedFiles` (returns the matching
///   `ChangedFile` so the caller can colour the badge by add/modify/delete).
/// - `unchanged` — a `file`/`class` target with a path that isn't in the set.
/// - `unknown` — no changed-file data, or a target with no addressable path
///   (`diff`, `note`, `atlas`, …). The caller hides the badge for `unknown`.
export interface ChangedBadge {
  status: 'changed' | 'unchanged' | 'unknown';
  file?: ChangedFile;
}

/// Resolve the changed/unchanged badge for a step relative to the tour's
/// diff ref. `changedFiles` is the cached result of `list_changes_since`
/// for that ref; pass an empty array (or the default) when git data is
/// unavailable — the result is then always `unknown` and the caller shows
/// no badge.
export function changedBadgeFor(
  t: WalkthroughStep['target'] | undefined,
  changedFiles: readonly ChangedFile[] = [],
): ChangedBadge {
  if (!t || changedFiles.length === 0) return { status: 'unknown' };
  const path = t.kind === 'file' ? t.path : undefined;
  // Only file targets carry a concrete path we can look up. Class targets
  // reference an FQN, not a file, so they stay `unknown` here.
  if (!path) return { status: 'unknown' };
  const match = changedFiles.find((c) => samePath(path, c.path));
  return match ? { status: 'changed', file: match } : { status: 'unchanged' };
}

/// Short status glyph for a `ChangedFile` — drives the badge letter and,
/// via a class, its colour. `?` for the catch-all `other`/`type_change`.
export function changedStatusGlyph(status: ChangedFile['status']): string {
  switch (status) {
    case 'added':
      return 'A';
    case 'modified':
      return 'M';
    case 'deleted':
      return 'D';
    case 'renamed':
      return 'R';
    default:
      return '?';
  }
}

/// One dot in the file-progress trail. `active` marks the file the current
/// step is looking at (highlighted dot); the caller uses `path`/`status`
/// for the hover tooltip and click-to-open.
export interface TrailDot {
  path: string;
  status: ChangedFile['status'];
  active: boolean;
}

/// Build the dot-trail through the tour's affected files. Each changed
/// file becomes one dot in stable order; the dot whose path matches the
/// current step's file target is flagged `active`. Empty when there is no
/// change data. Caps at `max` dots (default 24) so a huge changeset can't
/// blow out the strip — the active dot is always kept when it would fall
/// past the cap.
export function fileTrailFor(
  t: WalkthroughStep['target'] | undefined,
  changedFiles: readonly ChangedFile[] = [],
  max = 24,
): TrailDot[] {
  if (changedFiles.length === 0) return [];
  const activePath = t && t.kind === 'file' ? t.path : undefined;
  const dots: TrailDot[] = changedFiles.map((c) => ({
    path: c.path,
    status: c.status,
    active: activePath !== undefined && samePath(activePath, c.path),
  }));
  if (dots.length <= max) return dots;
  const activeIdx = dots.findIndex((d) => d.active);
  // Keep the head of the list; if the active dot is past the cap, swap the
  // last kept slot for it so the user still sees where they are.
  const kept = dots.slice(0, max);
  if (activeIdx >= max) kept[max - 1] = dots[activeIdx];
  return kept;
}

/// Single-letter glyph displayed next to the breadcrumb.
export function compassIconFor(t: WalkthroughStep['target'] | undefined): string {
  if (!t) return '';
  switch (t.kind) {
    case 'class':
      return 'C';
    case 'file':
      return 'F';
    case 'diff':
      return 'Δ';
    case 'risk':
      return 'R';
    case 'pattern':
      return 'P';
    case 'atlas':
      return '▦';
    case 'diagram-diff':
      return 'Δ';
    case 'note':
      return '·';
    default:
      return '';
  }
}
