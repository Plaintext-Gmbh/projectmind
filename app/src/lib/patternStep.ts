// Pure logic for the Walkthrough 2.0 `pattern` step (Cockpit 2.4, #160).
//
// A pattern step carries a `scope` string in the tour JSON. This module turns
// it into the module filter the pattern_check backend expects and keeps the
// parsing unit-testable, away from the Svelte component.

import { PATTERN_CONFIDENCE_FLOOR, type PatternViolation } from './api';

/**
 * Parse a walkthrough `scope` string into a module id.
 *
 *   `"module:auth"` → `"auth"`
 *   `"auth"`        → `"auth"`   (bare module id, tolerated)
 *   `"all"` / ``    → `null`     (whole repo)
 *
 * Unknown prefixes fall back to `null` (whole repo) rather than throwing, so a
 * malformed scope degrades to "check everything" instead of breaking the tour.
 */
export function moduleFromScope(scope?: string | null): string | null {
  if (!scope) return null;
  const trimmed = scope.trim();
  if (trimmed === '' || trimmed.toLowerCase() === 'all' || trimmed.toLowerCase() === 'repo') {
    return null;
  }
  const colon = trimmed.indexOf(':');
  if (colon === -1) return trimmed; // bare module id
  const prefix = trimmed.slice(0, colon).toLowerCase();
  const value = trimmed.slice(colon + 1).trim();
  if (prefix === 'module' && value !== '') return value;
  return null;
}

/**
 * Visible violations for a pattern step, sorted for the list: highest severity
 * first, then by file + line for stable ordering. Low-confidence hits are
 * dropped (noise floor), matching the heatmap's behaviour.
 */
export function stepViolations(violations: PatternViolation[]): PatternViolation[] {
  return violations
    .filter((v) => v.confidence >= PATTERN_CONFIDENCE_FLOOR)
    .slice()
    .sort(
      (a, b) =>
        b.severity - a.severity ||
        a.file.localeCompare(b.file) ||
        a.line - b.line,
    );
}

/** Short glyph for a severity level (1=info, 2=warn, 3=critical). */
export function severityGlyph(severity: number): string {
  if (severity >= 3) return '✗';
  if (severity === 2) return '⚠';
  return 'ℹ';
}
