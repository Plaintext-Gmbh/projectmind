/// Language-stats SVG renderer — extracted from `DiagramView.svelte`
/// (Viz-Katalog V1.3, #61). Horizontal-bar chart of file count per language.
/// Bars are sorted by file count desc (done server-side); bar width is
/// proportional to file count relative to the largest bucket. Pure.

import { esc, formatBytes } from './common';

export interface LanguageBucket {
  language: string;
  extension: string | null;
  files: number;
  bytes: number;
}

export interface LanguageStats {
  root: string;
  total_files: number;
  total_bytes: number;
  truncated: boolean;
  buckets: LanguageBucket[];
}

export function renderLanguageStats(stats: LanguageStats): string {
  if (stats.buckets.length === 0) {
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 900 480">
        <rect width="100%" height="100%" fill="#090d14"/>
        <text x="450" y="240" text-anchor="middle" fill="#94a3b8" font-size="18" font-family="ui-sans-serif,system-ui">No files found</text>
      </svg>`;
  }
  const PALETTE = [
    '#3b82f6',
    '#10b981',
    '#f59e0b',
    '#ef4444',
    '#8b5cf6',
    '#ec4899',
    '#14b8a6',
    '#f97316',
    '#22d3ee',
    '#a3e635',
    '#eab308',
    '#6366f1',
  ];
  const buckets = stats.buckets;
  const maxFiles = Math.max(1, ...buckets.map((b) => b.files));
  const totalFiles = Math.max(1, stats.total_files);
  const ROW = 28;
  const TOP = 60;
  const LEFT_LABEL = 200;
  const BAR_LEFT = LEFT_LABEL + 12;
  const RIGHT_PADDING = 160;
  const width = 960;
  const barTrack = width - BAR_LEFT - RIGHT_PADDING;
  const height = TOP + buckets.length * ROW + 40;
  const truncatedNote = stats.truncated
    ? `<text x="24" y="${height - 18}" class="caption">truncated at ${stats.total_files} files</text>`
    : '';
  const totalLine = formatBytes(stats.total_bytes);
  const rows = buckets
    .map((b, i) => {
      const w = Math.max(2, Math.round((b.files / maxFiles) * barTrack));
      const y = TOP + i * ROW;
      const color = PALETTE[i % PALETTE.length];
      const pct = ((b.files / totalFiles) * 100).toFixed(1);
      const extLabel = b.extension ? `.${b.extension}` : '—';
      return `<g class="row">
          <text x="${LEFT_LABEL}" y="${y + 14}" class="lang" text-anchor="end">${esc(b.language)}</text>
          <text x="${LEFT_LABEL - 8}" y="${y + 14}" class="ext" text-anchor="end" dx="-44">${esc(extLabel)}</text>
          <rect x="${BAR_LEFT}" y="${y + 4}" width="${w}" height="${ROW - 10}" rx="3" fill="${color}" opacity="0.85"/>
          <text x="${BAR_LEFT + w + 8}" y="${y + 14}" class="value">${b.files} · ${pct}% · ${formatBytes(b.bytes)}</text>
        </g>`;
    })
    .join('');
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}">
      <style>
        text{fill:#dce3f0;font:13px ui-sans-serif,system-ui,sans-serif}
        .title{font-size:18px;font-weight:600}
        .caption{font-size:11px;fill:#94a3b8}
        .lang{font-weight:600}
        .ext{font-family:ui-monospace,monospace;font-size:11px;fill:#94a3b8}
        .value{font-family:ui-monospace,monospace;font-size:11px;fill:#cbd5e1}
        .ruler{stroke:#1f2937;stroke-width:1}
      </style>
      <rect width="100%" height="100%" fill="#090d14"/>
      <text x="24" y="32" class="title">Sprachenverteilung</text>
      <text x="24" y="50" class="caption">${stats.total_files} Dateien · ${totalLine} · ${buckets.length} Buckets</text>
      <line x1="${BAR_LEFT}" y1="${TOP - 4}" x2="${BAR_LEFT}" y2="${TOP + buckets.length * ROW}" class="ruler"/>
      ${rows}
      ${truncatedNote}
    </svg>`;
}
