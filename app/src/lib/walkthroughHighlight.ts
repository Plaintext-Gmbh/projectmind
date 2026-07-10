/// Auto-scroll target selection for walkthrough steps (#175).
///
/// Pure function: given a step's highlight ranges, pick the one the
/// source pane should scroll to on step activation. Lifted out of
/// `WalkthroughView.svelte` so vitest can pin the selection rule
/// without booting the component (same pattern as `diffFocus.ts`).

import type { LineRange } from './api';

/// The range the pane auto-scrolls to: the one starting at the
/// smallest line — authors don't always emit ranges sorted, and the
/// reader expects to land on the topmost highlighted block. Returns
/// `null` when the step carries no highlights (caller shows the
/// target from the top instead).
export function firstHighlightRange(
  ranges: readonly LineRange[] | undefined | null,
): LineRange | null {
  if (!ranges || ranges.length === 0) return null;
  let first = ranges[0];
  for (const r of ranges) {
    if (r.from < first.from) first = r;
  }
  return first;
}
