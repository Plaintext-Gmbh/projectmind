import { describe, expect, it } from 'vitest';
import type { RiskScore } from './api';
import { colorForScore, shortName, tileValue, treemap } from './treemap';

function score(fqn: string, sloc: number, sc = 50): RiskScore {
  return {
    fqn,
    module: 'm',
    file: `m/${fqn}.java`,
    score: sc,
    churn: 1,
    cx: 1,
    sloc,
    cov: null,
    fan_in: 0,
    fan_out: 0,
    why: '',
  };
}

describe('treemap (atlas step, #160)', () => {
  it('returns no rects for an empty list or zero canvas', () => {
    expect(treemap([], 0, 0, 100, 100)).toEqual([]);
    expect(treemap([score('A', 10)], 0, 0, 0, 100)).toEqual([]);
  });

  it('fills the whole canvas with a single tile', () => {
    expect(treemap([score('A', 10)], 0, 0, 200, 100)).toEqual([
      { x: 0, y: 0, w: 200, h: 100, item: expect.objectContaining({ fqn: 'A' }) },
    ]);
  });

  it('splits area proportionally to SLOC and covers the canvas', () => {
    const rects = treemap([score('A', 75), score('B', 25)], 0, 0, 100, 40);
    // A is 3× B by SLOC, split horizontally (w >= h).
    expect(rects).toMatchInlineSnapshot(`
      [
        {
          "h": 40,
          "item": {
            "churn": 1,
            "cov": null,
            "cx": 1,
            "fan_in": 0,
            "fan_out": 0,
            "file": "m/A.java",
            "fqn": "A",
            "module": "m",
            "score": 50,
            "sloc": 75,
            "why": "",
          },
          "w": 75,
          "x": 0,
          "y": 0,
        },
        {
          "h": 40,
          "item": {
            "churn": 1,
            "cov": null,
            "cx": 1,
            "fan_in": 0,
            "fan_out": 0,
            "file": "m/B.java",
            "fqn": "B",
            "module": "m",
            "score": 50,
            "sloc": 25,
            "why": "",
          },
          "w": 25,
          "x": 75,
          "y": 0,
        },
      ]
    `);
  });

  it('treats zero-SLOC tiles as weight 1 so they still render', () => {
    expect(tileValue(score('Z', 0))).toBe(1);
  });
});

describe('colorForScore', () => {
  it('scales green (0) → red (100) and clamps', () => {
    expect(colorForScore(0)).toBe('hsl(120, 65%, 42%)');
    expect(colorForScore(100)).toBe('hsl(0, 65%, 42%)');
    expect(colorForScore(200)).toBe('hsl(0, 65%, 42%)');
  });
});

describe('shortName', () => {
  it('takes the last dotted segment', () => {
    expect(shortName('a.b.C')).toBe('C');
    expect(shortName('Bare')).toBe('Bare');
  });
});
