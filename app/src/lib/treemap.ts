// Pure treemap layout for the Risk Atlas (Cockpit 2.1) and the Walkthrough
// 2.0 `atlas` step (Cockpit 2.4, #160). Extracted from RiskAtlasView.svelte so
// both the standalone view and the walkthrough step render identical tiles and
// so the layout can be unit-tested.

import type { RiskScore } from './api';

export interface TreemapRect {
  x: number;
  y: number;
  w: number;
  h: number;
  item: RiskScore;
}

/** Tile weight — area is proportional to SLOC (min 1 so tiny classes show). */
export function tileValue(it: RiskScore): number {
  return Math.max(it.sloc, 1);
}

/**
 * Area-true treemap by alternating halving on cumulative SLOC. Deliberately
 * simple + correct; a squarified aspect-ratio pass is a possible follow-up.
 * Callers should pass `items` pre-sorted (largest first) for stable layout.
 */
export function treemap(
  items: RiskScore[],
  x: number,
  y: number,
  w: number,
  h: number,
): TreemapRect[] {
  if (items.length === 0 || w <= 0 || h <= 0) return [];
  if (items.length === 1) return [{ x, y, w, h, item: items[0] }];
  const total = items.reduce((s, it) => s + tileValue(it), 0);
  let acc = 0;
  let split = 1;
  for (let i = 0; i < items.length - 1; i++) {
    acc += tileValue(items[i]);
    if (acc >= total / 2) {
      split = i + 1;
      break;
    }
  }
  const a = items.slice(0, split);
  const b = items.slice(split);
  const frac = a.reduce((s, it) => s + tileValue(it), 0) / total;
  if (w >= h) {
    const aw = w * frac;
    return [...treemap(a, x, y, aw, h), ...treemap(b, x + aw, y, w - aw, h)];
  }
  const ah = h * frac;
  return [...treemap(a, x, y, w, ah), ...treemap(b, x, y + ah, w, h - ah)];
}

/** Colour scale: score 0 (green) → 100 (red). */
export function colorForScore(score: number): string {
  const s = Math.max(0, Math.min(100, score));
  const hue = 120 - (s / 100) * 120;
  return `hsl(${hue}, 65%, 42%)`;
}

/** Simple name from an fqn (last dotted segment). */
export function shortName(fqn: string): string {
  const i = fqn.lastIndexOf('.');
  return i >= 0 ? fqn.slice(i + 1) : fqn;
}
