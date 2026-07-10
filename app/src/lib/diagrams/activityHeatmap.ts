/// Activity-heatmap SVG renderer — extracted from `DiagramView.svelte`
/// (Viz-Katalog V1.3, #61). GitHub-style 7×N calendar grid: one column per
/// week, one row per weekday (Mo top). Cell colour ramps linearly with
/// commits-per-day relative to the busiest day. A side panel lists totals +
/// the top-10 authors. Pure.

import { esc } from './common';

export interface ActivityAuthorSlice {
  name: string;
  commits: number;
}
export interface ActivityDay {
  date: string;
  commits: number;
  top_authors: ActivityAuthorSlice[];
}
export interface ActivityAuthorTotals {
  name: string;
  commits: number;
}
export interface ActivityHeatmap {
  root: string;
  start_date: string;
  end_date: string;
  days: ActivityDay[];
  total_commits: number;
  distinct_authors: number;
  top_authors: ActivityAuthorTotals[];
  max_commits_per_day: number;
  longest_streak_days: number;
  truncated: boolean;
  no_git: boolean;
}

export function renderActivityHeatmap(heat: ActivityHeatmap): string {
  const CELL = 13;
  const GAP = 3;
  const LEFT = 60;
  const TOP = 90;
  const PANEL = 240;

  // Layout: bucket days into 7-row columns starting on the first
  // ISO-Monday at or before the start_date.
  const days = heat.days;
  if (days.length === 0) {
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1100 360">
        <rect width="100%" height="100%" fill="#090d14"/>
        <text x="550" y="180" text-anchor="middle" fill="#94a3b8" font-size="18">Keine Aktivität verfügbar.</text>
      </svg>`;
  }

  // weekday(0=Mo … 6=So) for each day, derived from the date string.
  function weekdayMondayBased(iso: string): number {
    const d = new Date(iso + 'T00:00:00Z');
    const w = d.getUTCDay(); // 0=Sun..6=Sat
    return (w + 6) % 7; // 0=Mon..6=Sun
  }

  const first = days[0];
  const firstW = weekdayMondayBased(first.date);
  // Number of leading empty cells in column 0.
  const totalCells = firstW + days.length;
  const columns = Math.ceil(totalCells / 7);
  const W = LEFT + columns * (CELL + GAP) + 16 + PANEL + 24;
  const H = TOP + 7 * (CELL + GAP) + 80;

  const max = Math.max(1, heat.max_commits_per_day);
  const ramp = (n: number): string => {
    if (n === 0) return '#1f2937';
    const t = Math.min(1, n / max);
    // 5 buckets, dark → bright green
    if (t < 0.2) return '#0e3b27';
    if (t < 0.45) return '#1c6c45';
    if (t < 0.7) return '#2ea264';
    if (t < 0.9) return '#3fcf83';
    return '#7ee787';
  };

  const cells = days
    .map((d, i) => {
      const idx = firstW + i;
      const col = Math.floor(idx / 7);
      const row = idx % 7;
      const x = LEFT + col * (CELL + GAP);
      const y = TOP + row * (CELL + GAP);
      const top = d.top_authors
        .map((a) => `${a.name} · ${a.commits}`)
        .join('\n');
      const tip = `${d.date} — ${d.commits} ${d.commits === 1 ? 'Commit' : 'Commits'}${top ? `\n${top}` : ''}`;
      return `<rect x="${x}" y="${y}" width="${CELL}" height="${CELL}" rx="2" fill="${ramp(d.commits)}"><title>${esc(tip)}</title></rect>`;
    })
    .join('');

  // Weekday labels (Mo, Mi, Fr).
  const weekdayLabels = ['Mo', '', 'Mi', '', 'Fr', '', 'So']
    .map((lbl, i) => `<text x="${LEFT - 8}" y="${TOP + i * (CELL + GAP) + 11}" text-anchor="end" class="wd">${lbl}</text>`)
    .join('');

  // Month labels: one per first-Monday-of-month visible.
  const monthLabels: string[] = [];
  let lastMonth = '';
  for (let i = 0; i < days.length; i += 1) {
    const month = days[i].date.slice(0, 7);
    if (month !== lastMonth) {
      const idx = firstW + i;
      const col = Math.floor(idx / 7);
      const x = LEFT + col * (CELL + GAP);
      const m = days[i].date.slice(5, 7);
      const names = ['Jan', 'Feb', 'Mär', 'Apr', 'Mai', 'Jun', 'Jul', 'Aug', 'Sep', 'Okt', 'Nov', 'Dez'];
      monthLabels.push(`<text x="${x}" y="${TOP - 8}" class="mlbl">${names[parseInt(m, 10) - 1] ?? m}</text>`);
      lastMonth = month;
    }
  }

  // Side panel: stats + top authors.
  const panelX = LEFT + columns * (CELL + GAP) + 24;
  const noGit = heat.no_git
    ? `<text x="${panelX}" y="${TOP + 16}" class="warn">Keine Git-Historie verfügbar.</text>`
    : '';
  const stats = !heat.no_git
    ? `
        <text x="${panelX}" y="${TOP}" class="panel-title">Übersicht</text>
        <text x="${panelX}" y="${TOP + 22}" class="panel-line">Zeitraum: ${heat.start_date} – ${heat.end_date}</text>
        <text x="${panelX}" y="${TOP + 40}" class="panel-line">Commits gesamt: ${heat.total_commits}</text>
        <text x="${panelX}" y="${TOP + 58}" class="panel-line">Aktivste Tage: max. ${heat.max_commits_per_day}</text>
        <text x="${panelX}" y="${TOP + 76}" class="panel-line">Längste Streak: ${heat.longest_streak_days} Tage</text>
        <text x="${panelX}" y="${TOP + 94}" class="panel-line">Distincte Autoren: ${heat.distinct_authors}</text>
        ${heat.truncated ? `<text x="${panelX}" y="${TOP + 112}" class="panel-warn">Walk bei ${heat.total_commits} Commits gestoppt.</text>` : ''}`
    : '';
  const authors = heat.top_authors
    .map((a, i) => `<text x="${panelX}" y="${TOP + 140 + i * 16}" class="panel-line">${i + 1}. ${esc(a.name)} — ${a.commits}</text>`)
    .join('');
  const authorTitle = heat.top_authors.length > 0
    ? `<text x="${panelX}" y="${TOP + 124}" class="panel-title">Top-Autoren</text>`
    : '';

  // Legend strip beneath the grid.
  const legendY = TOP + 7 * (CELL + GAP) + 28;
  const legend = ['#1f2937', '#0e3b27', '#1c6c45', '#2ea264', '#3fcf83', '#7ee787']
    .map((c, i) => `<rect x="${LEFT + i * (CELL + GAP)}" y="${legendY}" width="${CELL}" height="${CELL}" rx="2" fill="${c}"/>`)
    .join('');

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}">
      <style>
        text{fill:#dce3f0;font:13px ui-sans-serif,system-ui,sans-serif}
        .title{font-size:20px;font-weight:600}
        .subtitle{font-size:12px;fill:#94a3b8}
        .wd{font-size:10px;font-family:ui-monospace,monospace;fill:#94a3b8}
        .mlbl{font-size:10px;font-family:ui-monospace,monospace;fill:#94a3b8}
        .panel-title{font-size:13px;font-weight:600;fill:#cbd5e1}
        .panel-line{font-size:11px;font-family:ui-monospace,monospace;fill:#94a3b8}
        .panel-warn{font-size:10px;font-family:ui-monospace,monospace;fill:#fbbf24}
        .warn{font-size:13px;fill:#fbbf24}
        .legend{font-size:10px;font-family:ui-monospace,monospace;fill:#94a3b8}
      </style>
      <rect width="100%" height="100%" fill="#090d14"/>
      <text x="24" y="40" class="title">Commit-Aktivität</text>
      <text x="24" y="60" class="subtitle">Letzte 12 Monate · ${heat.total_commits} Commits · ${heat.distinct_authors} Autoren</text>
      ${monthLabels.join('')}
      ${weekdayLabels}
      ${cells}
      <text x="${LEFT - 8}" y="${legendY + 11}" text-anchor="end" class="legend">weniger</text>
      ${legend}
      <text x="${LEFT + 6 * (CELL + GAP) + CELL + 8}" y="${legendY + 11}" class="legend">mehr</text>
      ${noGit}
      ${stats}
      ${authorTitle}
      ${authors}
    </svg>`;
}
