// Pure logic for the Pattern Lens compliance heatmap (Cockpit 2.3, #159).
// Kept separate from PatternsView.svelte so it can be unit-tested in the
// project's vitest style (component rendering is not tested here).

import {
  PATTERN_CONFIDENCE_FLOOR,
  type PatternResult,
  type PatternViolation,
} from './api';

/** Compliance state of one (pattern × module) heatmap cell. */
export type CellState = 'pass' | 'warn' | 'fail' | 'na';

export interface HeatmapCell {
  pattern: string;
  module: string;
  state: CellState;
  /** Number of classes that satisfy the rule in this module. */
  holds: number;
  /** Number of *visible* (confidence ≥ floor) violations in this module. */
  violations: number;
  /** The visible violations, for the drill-in panel. */
  violationList: PatternViolation[];
  /** Glyph for the cell: ✓✓ / ⚠ / ✗ / – */
  glyph: string;
}

export interface HeatmapRow {
  pattern: string;
  confidence: number;
  cells: HeatmapCell[];
}

export interface Heatmap {
  modules: string[];
  rows: HeatmapRow[];
}

/** Violations at or above the noise floor — the only ones the heatmap shows. */
export function visibleViolations(result: PatternResult): PatternViolation[] {
  return result.violations.filter((v) => v.confidence >= PATTERN_CONFIDENCE_FLOOR);
}

function glyphFor(state: CellState): string {
  switch (state) {
    case 'pass':
      return '✓✓';
    case 'warn':
      return '⚠';
    case 'fail':
      return '✗';
    default:
      return '–';
  }
}

/**
 * Decide a cell's compliance state from its counts.
 * - no violations, some holds  → pass (✓✓)
 * - violations present         → fail (✗) if they outnumber holds, else warn (⚠)
 * - neither holds nor viols    → n/a (the pattern doesn't apply in that module)
 */
export function cellState(holds: number, violations: number): CellState {
  if (violations === 0) return holds > 0 ? 'pass' : 'na';
  return violations >= holds ? 'fail' : 'warn';
}

/** Union of every module id that appears in any detector's holds/violations. */
export function collectModules(results: PatternResult[]): string[] {
  const set = new Set<string>();
  for (const r of results) {
    for (const h of r.holds) set.add(h.module);
    for (const v of visibleViolations(r)) set.add(v.module);
  }
  return [...set].sort();
}

/**
 * Build the full compliance heatmap from the per-detector results.
 * Rows = patterns, columns = modules. Low-confidence violations are already
 * filtered out (noise suppression, issue #159).
 */
export function buildHeatmap(results: PatternResult[]): Heatmap {
  const modules = collectModules(results);
  const rows: HeatmapRow[] = results.map((r) => {
    const holdsByModule = new Map<string, number>();
    for (const h of r.holds) holdsByModule.set(h.module, h.count);

    const violsByModule = new Map<string, PatternViolation[]>();
    for (const v of visibleViolations(r)) {
      const list = violsByModule.get(v.module) ?? [];
      list.push(v);
      violsByModule.set(v.module, list);
    }

    const cells: HeatmapCell[] = modules.map((module) => {
      const holds = holdsByModule.get(module) ?? 0;
      const violationList = violsByModule.get(module) ?? [];
      const violations = violationList.length;
      const state = cellState(holds, violations);
      return {
        pattern: r.pattern,
        module,
        state,
        holds,
        violations,
        violationList,
        glyph: glyphFor(state),
      };
    });

    return { pattern: r.pattern, confidence: r.confidence, cells };
  });

  return { modules, rows };
}
