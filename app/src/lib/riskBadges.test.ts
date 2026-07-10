import { describe, expect, it } from 'vitest';
import type { RiskScore } from './api';
import { formatBadgeRow, riskBadges, riskTier } from './riskBadges';

function score(over: Partial<RiskScore> = {}): RiskScore {
  return {
    fqn: 'a.b.C',
    module: 'core',
    file: 'core/C.java',
    score: 50,
    churn: 87,
    cx: 24,
    sloc: 400,
    cov: 0.12,
    fan_in: 5,
    fan_out: 3,
    why: 'hot+complex',
    ...over,
  };
}

describe('riskBadges (auto-annotation, #160)', () => {
  it('renders churn · cov · cx by default', () => {
    const badges = riskBadges(score());
    // Snapshot the compact row an author never has to write themselves.
    expect(formatBadgeRow(badges)).toMatchInlineSnapshot(`"churn 87 · cov 12% · cx 24"`);
  });

  it('honours an explicit show order and adds fan-in/out', () => {
    const badges = riskBadges(score(), ['cx', 'churn', 'fan_in', 'fan_out']);
    expect(formatBadgeRow(badges)).toMatchInlineSnapshot(
      `"cx 24 · churn 87 · fan-in 5 · fan-out 3"`,
    );
  });

  it('drops signals without data (no "cov —")', () => {
    const badges = riskBadges(score({ cov: null, churn: 0 }));
    // churn 0 and cov null both fall away; only cx remains.
    expect(formatBadgeRow(badges)).toBe('cx 24');
  });

  it('ignores unknown show entries but keeps valid ones', () => {
    const badges = riskBadges(score(), ['bogus', 'churn']);
    expect(badges.map((b) => b.id)).toEqual(['churn']);
  });

  it('rounds coverage to a whole percentage', () => {
    expect(formatBadgeRow(riskBadges(score({ cov: 0.126 }), ['cov']))).toBe('cov 13%');
    expect(formatBadgeRow(riskBadges(score({ cov: 1 }), ['cov']))).toBe('cov 100%');
  });

  it('buckets the composite score into tiers', () => {
    expect(riskTier(10)).toBe('cool');
    expect(riskTier(50)).toBe('warm');
    expect(riskTier(90)).toBe('hot');
  });
});
