// Pure logic for the Walkthrough 2.0 risk badges (Cockpit 2.4, #160).
//
// Two consumers:
//   * `class`-kind steps get a non-intrusive auto-annotation badge row
//     (`churn 87 · cov 12% · cx 24`) whenever atlas data resolves for the
//     step's fqn. Zero author effort.
//   * `risk`-kind steps render the same badges as an explicit header bar,
//     honouring the step's `show:[...]` filter.
//
// Kept separate from the Svelte components so it can be unit-tested in the
// project's vitest style (component rendering is not tested here).

import type { RiskScore } from './api';

/** One rendered badge: a short label plus its formatted value. */
export interface RiskBadge {
  /** Signal id — matches the `show` entries and RiskScore fields. */
  id: 'churn' | 'cx' | 'cov' | 'fan_in' | 'fan_out';
  /** Human label shown before the value (e.g. `churn`, `cov`, `cx`). */
  label: string;
  /** Formatted value (`87`, `12%`, `24`). */
  value: string;
}

/** Signals shown by default (auto-annotation) in this order. */
export const DEFAULT_SIGNALS: RiskBadge['id'][] = ['churn', 'cov', 'cx'];

/** Every signal a risk step can surface, in render order. */
export const ALL_SIGNALS: RiskBadge['id'][] = ['churn', 'cov', 'cx', 'fan_in', 'fan_out'];

const LABELS: Record<RiskBadge['id'], string> = {
  churn: 'churn',
  cx: 'cx',
  cov: 'cov',
  fan_in: 'fan-in',
  fan_out: 'fan-out',
};

/** `true` when the signal carries usable data on this score. */
function hasData(score: RiskScore, id: RiskBadge['id']): boolean {
  switch (id) {
    case 'cov':
      // Coverage is `null` when no report resolves for the class.
      return score.cov !== null && score.cov !== undefined;
    case 'churn':
      return score.churn > 0;
    case 'cx':
      return score.cx > 0;
    case 'fan_in':
      return score.fan_in > 0;
    case 'fan_out':
      return score.fan_out > 0;
  }
}

function format(score: RiskScore, id: RiskBadge['id']): string {
  switch (id) {
    case 'cov':
      // `cov` is a 0..=1 fraction; show as a whole-number percentage.
      return `${Math.round((score.cov ?? 0) * 100)}%`;
    case 'churn':
      return String(score.churn);
    case 'cx':
      return String(score.cx);
    case 'fan_in':
      return String(score.fan_in);
    case 'fan_out':
      return String(score.fan_out);
  }
}

/**
 * Build the badge row for a class/risk step.
 *
 * `show` selects and orders the signals; unknown entries are ignored. An
 * empty/omitted `show` falls back to {@link DEFAULT_SIGNALS}. Signals without
 * data are dropped so a badge row never reads `cov —`.
 */
export function riskBadges(score: RiskScore, show?: string[] | null): RiskBadge[] {
  const requested = (show && show.length > 0 ? show : DEFAULT_SIGNALS).filter((s): s is RiskBadge['id'] =>
    (ALL_SIGNALS as string[]).includes(s),
  );
  const badges: RiskBadge[] = [];
  for (const id of requested) {
    if (!hasData(score, id)) continue;
    badges.push({ id, label: LABELS[id], value: format(score, id) });
  }
  return badges;
}

/** Render badges as the compact `churn 87 · cov 12% · cx 24` string. */
export function formatBadgeRow(badges: RiskBadge[]): string {
  return badges.map((b) => `${b.label} ${b.value}`).join(' · ');
}

/**
 * A risk `score` (0..=100) bucketed into a coarse tier for the header bar's
 * colour + width. Mirrors the treemap's hot/warm/cool banding.
 */
export function riskTier(score: number): 'hot' | 'warm' | 'cool' {
  if (score >= 66) return 'hot';
  if (score >= 33) return 'warm';
  return 'cool';
}
