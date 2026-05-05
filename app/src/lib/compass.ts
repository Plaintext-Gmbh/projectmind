/// Change-compass orientation helpers (#127).
///
/// The walkthrough viewer renders a single-line strip below each step's
/// target hint that shows *where this step sits in the codebase*. The
/// helpers here keep the logic out of the Svelte component so vitest can
/// pin the breadcrumb math without booting a DOM.
import type { WalkthroughStep } from './api';

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
  return [];
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
    case 'note':
      return '·';
    default:
      return '';
  }
}
