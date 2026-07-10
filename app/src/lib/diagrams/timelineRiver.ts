/// Timeline-river SVG renderer (#63 concept 4). A horizontal time axis with
/// `now` on the right and the window edge on the left; one horizontal band
/// per module; each commit is a "drop" placed at its age on the time axis.
/// Answers "when did module X go active / quiet" at a glance.
///
/// Pure: `renderTimelineRiver(payload, opts): string` returns an SVG string —
/// no DOM, no Svelte — so the layout maths is unit-testable in isolation,
/// like the other extracted renderers under `lib/diagrams/`.
///
/// Time axis is LOGARITHMIC (log(1 + secs_ago)). On an active repo most
/// commits cluster in the last few weeks; a linear axis would smear them
/// against the right edge and leave months of empty space. Log spreads the
/// recent past out and compresses the distant past, which is exactly the
/// resolution a "who's-been-active-lately" view wants. `now` (secs_ago = 0)
/// maps to the right edge; `window_secs` maps to the left edge.

import { esc } from './common';

export interface CommitDrop {
  secs_ago: number;
  sha: string;
  summary: string;
}

export interface ModuleActivity {
  module: string;
  commits: CommitDrop[];
}

export interface CommitActivity {
  root: string;
  now_secs: number;
  window_secs: number;
  modules: ModuleActivity[];
  total_commits: number;
  truncated: boolean;
  no_git: boolean;
}

export interface TimelineRiverOptions {
  /// Total SVG width in px. Defaults to a wide-ish canvas the viewport
  /// store then fits to the stage.
  width?: number;
}

// Layout constants (exported are the ones the tests pin).
const LEFT = 150; // module-label gutter
const RIGHT_PAD = 24;
const TOP = 56; // room for the axis + title
const BAND_H = 34; // vertical space per module band
const DROP_R = 3.2; // commit-drop radius
const DEFAULT_W = 1100;

/// Map an age (seconds ago) to an x coordinate. Monotonic decreasing in
/// `secsAgo`: 0 → right edge (`plotRight`), `windowSecs` → left edge
/// (`plotLeft`). Ages beyond the window are clamped to the left edge so a
/// stray old drop never lands in the label gutter.
///
/// Exported for unit tests (monotonicity + endpoint anchoring).
export function timeX(
  secsAgo: number,
  windowSecs: number,
  plotLeft: number,
  plotRight: number,
): number {
  const w = Math.max(1, windowSecs);
  const clamped = Math.min(Math.max(secsAgo, 0), w);
  // log(1 + age) normalised to [0, 1]; 0 = now, 1 = window edge.
  const t = Math.log1p(clamped) / Math.log1p(w);
  // now (t = 0) → right edge; window edge (t = 1) → left edge.
  return plotRight - t * (plotRight - plotLeft);
}

/// Vertical centre of the band for the module at row `index` (0-based).
/// Exported for unit tests.
export function bandY(index: number): number {
  return TOP + index * BAND_H + BAND_H / 2;
}

/// Deterministic vertical jitter within a band so overlapping drops at the
/// same age don't fully occlude each other. Pure function of the sha so the
/// same commit always lands in the same spot across re-renders.
function jitter(sha: string): number {
  let h = 0;
  for (let i = 0; i < sha.length; i++) {
    h = (h * 31 + sha.charCodeAt(i)) & 0xffff;
  }
  // Map to roughly ±(BAND_H/2 - drop) so drops stay inside the band.
  const span = BAND_H / 2 - DROP_R - 2;
  return ((h / 0xffff) * 2 - 1) * span;
}

/// Axis ticks: a handful of human-readable ages from `now` back to the
/// window edge. Each returns [secsAgo, label].
function axisTicks(windowSecs: number): Array<[number, string]> {
  const DAY = 86_400;
  const candidates: Array<[number, string]> = [
    [0, 'now'],
    [7 * DAY, '1w'],
    [30 * DAY, '1mo'],
    [90 * DAY, '3mo'],
    [180 * DAY, '6mo'],
    [365 * DAY, '1y'],
    [730 * DAY, '2y'],
  ];
  return candidates.filter(([secs]) => secs <= windowSecs);
}

/// Render the timeline river to an SVG string.
export function renderTimelineRiver(
  data: CommitActivity,
  opts: TimelineRiverOptions = {},
): string {
  const W = opts.width ?? DEFAULT_W;
  const plotLeft = LEFT;
  const plotRight = W - RIGHT_PAD;

  // Empty state: no git, or a repo with no activity in the window.
  if (data.no_git || data.modules.length === 0) {
    const msg = data.no_git ? 'Kein Git-Repository.' : 'Keine Commit-Aktivität im Zeitfenster.';
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} 200">
      <rect width="100%" height="100%" fill="#090d14"/>
      <text x="${W / 2}" y="100" text-anchor="middle" fill="#94a3b8" font-size="16">${esc(msg)}</text>
    </svg>`;
  }

  const rows = data.modules.length;
  const H = TOP + rows * BAND_H + 40;
  const ticks = axisTicks(data.window_secs);

  const parts: string[] = [];
  parts.push(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" font-family="system-ui, sans-serif">`,
  );
  parts.push(`<rect width="100%" height="100%" fill="#090d14"/>`);

  // Title + summary.
  const summary = `${data.total_commits} Commits · ${rows} Module${data.truncated ? ' · gekappt' : ''}`;
  parts.push(
    `<text x="${LEFT}" y="24" fill="#e2e8f0" font-size="15" font-weight="600">Timeline river</text>`,
  );
  parts.push(`<text x="${plotRight}" y="24" text-anchor="end" fill="#64748b" font-size="12">${esc(summary)}</text>`);

  // Axis grid: vertical tick lines + labels. "older ←" hint on the far left.
  const axisY0 = TOP - 8;
  const axisY1 = TOP + rows * BAND_H + 6;
  for (const [secs, label] of ticks) {
    const x = timeX(secs, data.window_secs, plotLeft, plotRight);
    parts.push(
      `<line x1="${x.toFixed(1)}" y1="${axisY0}" x2="${x.toFixed(1)}" y2="${axisY1}" stroke="#1e293b" stroke-width="1"/>`,
    );
    parts.push(
      `<text x="${x.toFixed(1)}" y="${axisY0 - 4}" text-anchor="middle" fill="#64748b" font-size="11">${esc(label)}</text>`,
    );
  }
  parts.push(
    `<text x="${plotLeft}" y="${axisY1 + 20}" fill="#475569" font-size="11">← älter</text>`,
  );
  parts.push(
    `<text x="${plotRight}" y="${axisY1 + 20}" text-anchor="end" fill="#475569" font-size="11">jetzt →</text>`,
  );

  // One band per module.
  data.modules.forEach((mod, i) => {
    const cy = bandY(i);
    // Band baseline + label.
    parts.push(
      `<line x1="${plotLeft}" y1="${cy.toFixed(1)}" x2="${plotRight}" y2="${cy.toFixed(1)}" stroke="#141c28" stroke-width="1"/>`,
    );
    const label = mod.module.length > 20 ? `${mod.module.slice(0, 19)}…` : mod.module;
    parts.push(
      `<text x="${LEFT - 12}" y="${(cy + 4).toFixed(1)}" text-anchor="end" fill="#cbd5e1" font-size="12">${esc(label)}</text>`,
    );
    // Freshness dot on the far left: green when active recently, dimmer when
    // the module's newest commit is old. `commits[0]` is newest.
    const newest = mod.commits[0]?.secs_ago ?? data.window_secs;
    const freshT = 1 - Math.min(1, Math.log1p(newest) / Math.log1p(Math.max(1, data.window_secs)));
    const hue = Math.round(140 * freshT); // 0 = grey-ish red, 140 = green
    parts.push(
      `<circle cx="${(LEFT - 132).toFixed(1)}" cy="${cy.toFixed(1)}" r="4" fill="hsl(${hue} 60% 45%)"><title>${esc(mod.module)}: newest commit ${humanAge(newest)}</title></circle>`,
    );

    // Commit drops.
    for (const c of mod.commits) {
      const x = timeX(c.secs_ago, data.window_secs, plotLeft, plotRight);
      const y = cy + jitter(c.sha);
      parts.push(
        `<circle cx="${x.toFixed(1)}" cy="${y.toFixed(1)}" r="${DROP_R}" fill="#38bdf8" fill-opacity="0.75"><title>${esc(c.sha)} · ${esc(humanAge(c.secs_ago))}\n${esc(c.summary)}</title></circle>`,
      );
    }
  });

  parts.push(`</svg>`);
  return parts.join('\n');
}

/// Compact human age for tooltips ("3d", "5mo", "1.2y").
function humanAge(secs: number): string {
  const DAY = 86_400;
  if (secs < DAY) return `${Math.max(1, Math.round(secs / 3600))}h`;
  const days = secs / DAY;
  if (days < 30) return `${Math.round(days)}d`;
  if (days < 365) return `${Math.round(days / 30)}mo`;
  return `${(days / 365).toFixed(1)}y`;
}
