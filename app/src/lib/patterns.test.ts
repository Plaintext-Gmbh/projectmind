import { describe, expect, it } from 'vitest';
import type { PatternResult, PatternViolation } from './api';
import {
  buildHeatmap,
  cellState,
  collectModules,
  visibleViolations,
} from './patterns';

function violation(module: string, confidence: number, line = 1): PatternViolation {
  return {
    module,
    file: `${module}/File.java`,
    line,
    fqn: `${module}.File`,
    message: 'drift',
    severity: 2,
    confidence,
  };
}

function result(pattern: string, over: Partial<PatternResult> = {}): PatternResult {
  return {
    pattern,
    holds: [],
    violations: [],
    confidence: 0.85,
    ...over,
  };
}

describe('visibleViolations', () => {
  it('hides violations below the 0.6 confidence floor', () => {
    const r = result('di_only', {
      violations: [violation('m', 0.9), violation('m', 0.4)],
    });
    expect(visibleViolations(r)).toHaveLength(1);
    expect(visibleViolations(r)[0].confidence).toBe(0.9);
  });

  it('keeps a violation exactly at the floor', () => {
    const r = result('layered', { violations: [violation('m', 0.6)] });
    expect(visibleViolations(r)).toHaveLength(1);
  });
});

describe('cellState', () => {
  it('passes when there are holds and no violations', () => {
    expect(cellState(3, 0)).toBe('pass');
  });
  it('is n/a when nothing applies', () => {
    expect(cellState(0, 0)).toBe('na');
  });
  it('warns when holds still outnumber violations', () => {
    expect(cellState(5, 1)).toBe('warn');
  });
  it('fails when violations reach or exceed holds', () => {
    expect(cellState(1, 1)).toBe('fail');
    expect(cellState(0, 2)).toBe('fail');
  });
});

describe('collectModules', () => {
  it('unions and sorts module ids across holds and visible violations', () => {
    const results = [
      result('repository', { holds: [{ module: 'b', count: 1 }] }),
      result('layered', { violations: [violation('a', 0.9), violation('c', 0.3)] }),
    ];
    // 'c' is only in a hidden (low-confidence) violation -> excluded.
    expect(collectModules(results)).toEqual(['a', 'b']);
  });
});

describe('buildHeatmap', () => {
  it('produces one row per pattern and one cell per module', () => {
    const results = [
      result('repository', {
        holds: [{ module: 'core', count: 4 }],
        violations: [violation('web', 0.9)],
      }),
      result('no_static_state', {
        holds: [
          { module: 'core', count: 2 },
          { module: 'web', count: 1 },
        ],
      }),
    ];
    const hm = buildHeatmap(results);
    expect(hm.modules).toEqual(['core', 'web']);
    expect(hm.rows).toHaveLength(2);

    const repoRow = hm.rows[0];
    const coreCell = repoRow.cells.find((c) => c.module === 'core')!;
    const webCell = repoRow.cells.find((c) => c.module === 'web')!;
    expect(coreCell.state).toBe('pass');
    expect(coreCell.glyph).toBe('✓✓');
    expect(webCell.state).toBe('fail');
    expect(webCell.violations).toBe(1);
    expect(webCell.violationList[0].module).toBe('web');

    const staticRow = hm.rows[1];
    // no_static_state has no data for a 'web'-only violation -> web holds=1 pass,
    // and no violation anywhere.
    const staticWeb = staticRow.cells.find((c) => c.module === 'web')!;
    expect(staticWeb.state).toBe('pass');
  });

  it('drops low-confidence violations from the cell counts', () => {
    const results = [
      result('di_only', {
        holds: [{ module: 'web', count: 3 }],
        violations: [violation('web', 0.5)],
      }),
    ];
    const hm = buildHeatmap(results);
    const cell = hm.rows[0].cells[0];
    expect(cell.violations).toBe(0);
    expect(cell.state).toBe('pass');
  });
});
