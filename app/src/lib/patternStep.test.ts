import { describe, expect, it } from 'vitest';
import type { PatternViolation } from './api';
import { moduleFromScope, severityGlyph, stepViolations } from './patternStep';

function violation(over: Partial<PatternViolation> = {}): PatternViolation {
  return {
    module: 'auth',
    file: 'auth/Foo.java',
    line: 10,
    fqn: 'auth.Foo',
    message: 'drift',
    severity: 2,
    confidence: 0.9,
    ...over,
  };
}

describe('moduleFromScope (pattern step, #160)', () => {
  it('parses the module:<id> form', () => {
    expect(moduleFromScope('module:auth')).toBe('auth');
  });

  it('tolerates a bare module id', () => {
    expect(moduleFromScope('auth')).toBe('auth');
  });

  it('maps all / repo / empty / null to whole-repo (null)', () => {
    expect(moduleFromScope('all')).toBeNull();
    expect(moduleFromScope('repo')).toBeNull();
    expect(moduleFromScope('')).toBeNull();
    expect(moduleFromScope(null)).toBeNull();
    expect(moduleFromScope(undefined)).toBeNull();
  });

  it('degrades unknown prefixes to whole-repo instead of throwing', () => {
    expect(moduleFromScope('package:a.b')).toBeNull();
    expect(moduleFromScope('module:')).toBeNull();
  });
});

describe('stepViolations (pattern step, #160)', () => {
  it('hides sub-floor confidence and sorts critical-first, then file:line', () => {
    const list = stepViolations([
      violation({ fqn: 'a.Low', confidence: 0.4, severity: 3 }), // hidden (noise)
      violation({ fqn: 'a.Warn', file: 'b.java', line: 5, severity: 2 }),
      violation({ fqn: 'a.Crit', file: 'a.java', line: 9, severity: 3 }),
      violation({ fqn: 'a.CritEarly', file: 'a.java', line: 2, severity: 3 }),
    ]);
    expect(list.map((v) => v.fqn)).toEqual(['a.CritEarly', 'a.Crit', 'a.Warn']);
  });
});

describe('severityGlyph', () => {
  it('maps severity levels to glyphs', () => {
    expect(severityGlyph(3)).toBe('✗');
    expect(severityGlyph(2)).toBe('⚠');
    expect(severityGlyph(1)).toBe('ℹ');
  });
});
