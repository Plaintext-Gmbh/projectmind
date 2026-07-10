/// Architecture-flow SVG renderer — extracted from `DiagramView.svelte`
/// (Viz-Katalog V1.3, #61). Horizontal layer bands stacked top→bottom; each
/// band shows its name, class chips coloured by stereotype, a stereotype
/// histogram and a class count. Aggregated cross-layer edges become
/// inter-band arrows whose stroke width encodes the relation count. Pure.

import { esc } from './common';

export interface FlowClass {
  fqn: string;
  name: string;
  module: string;
  stereotype: string | null;
}
export interface FlowLayer {
  id: string;
  label: string;
  description: string;
  color: string;
  classes: FlowClass[];
  stereotypes: Record<string, number>;
}
export interface FlowEdge {
  from: string;
  to: string;
  count: number;
}
export interface ArchitectureFlow {
  root: string;
  total_classes: number;
  total_modules: number;
  cross_module_edges: number;
  layers: FlowLayer[];
  edges: FlowEdge[];
}

export function renderArchitectureFlow(flow: ArchitectureFlow): string {
  const W = 1100;
  const PAD_X = 40;
  const PAD_TOP = 80;
  const PAD_BETWEEN = 20;
  const BAND_H = 150;
  const CHIP_H = 26;
  const CHIP_W = 132;
  const CHIPS_PER_ROW = Math.max(1, Math.floor((W - 2 * PAD_X - 16) / (CHIP_W + 8)));

  if (flow.total_classes === 0) {
    const H = 320;
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}">
        <rect width="100%" height="100%" fill="#090d14"/>
        <text x="${W / 2}" y="${H / 2}" text-anchor="middle" fill="#94a3b8" font-size="18" font-family="ui-sans-serif,system-ui">Keine Klassen erkannt — Architektur-Schichten benötigen geparsten Code.</text>
      </svg>`;
  }

  type Band = { layer: FlowLayer; y: number; h: number; rows: number };
  const bands: Band[] = [];
  let cursor = PAD_TOP;
  for (const layer of flow.layers) {
    const rows = layer.classes.length === 0 ? 1 : Math.ceil(layer.classes.length / CHIPS_PER_ROW);
    const h = Math.max(BAND_H, 56 + rows * (CHIP_H + 8));
    bands.push({ layer, y: cursor, h, rows });
    cursor += h + PAD_BETWEEN;
  }
  const H = cursor + 40;

  const bandSvg = bands
    .map(({ layer, y, h }) => {
      const total = layer.classes.length;
      const stripe = `<rect x="${PAD_X}" y="${y}" width="${W - 2 * PAD_X}" height="${h}" rx="14" ry="14" fill="${layer.color}" fill-opacity="0.08" stroke="${layer.color}" stroke-width="1.4"/>`;
      const accent = `<rect x="${PAD_X}" y="${y}" width="6" height="${h}" rx="3" fill="${layer.color}"/>`;
      const head = `
          <text x="${PAD_X + 22}" y="${y + 28}" class="band-title" fill="${layer.color}">${esc(layer.label)}</text>
          <text x="${PAD_X + 22}" y="${y + 48}" class="band-desc">${esc(layer.description)}</text>
          <text x="${W - PAD_X - 16}" y="${y + 28}" text-anchor="end" class="band-count" fill="${layer.color}">${total} ${total === 1 ? 'Klasse' : 'Klassen'}</text>`;

      const stereoEntries = Object.entries(layer.stereotypes).sort((a, b) => b[1] - a[1]);
      const stereoBadges = stereoEntries
        .slice(0, 4)
        .map(([s, n], i) => {
          const bx = W - PAD_X - 16 - (stereoEntries.length > 4 ? 110 : 0) - i * 86;
          return `<g><rect x="${bx - 78}" y="${y + 36}" width="78" height="18" rx="9" fill="${layer.color}" fill-opacity="0.18" stroke="${layer.color}" stroke-opacity="0.5"/><text x="${bx - 39}" y="${y + 49}" text-anchor="middle" class="stereo">${esc(s)} · ${n}</text></g>`;
        })
        .join('');

      const chips = layer.classes
        .map((c, i) => {
          const col = i % CHIPS_PER_ROW;
          const row = Math.floor(i / CHIPS_PER_ROW);
          const cx = PAD_X + 22 + col * (CHIP_W + 8);
          const cy = y + 60 + row * (CHIP_H + 8);
          const label = shortChipLabel(c.name);
          return `<g class="chip"><rect x="${cx}" y="${cy}" width="${CHIP_W}" height="${CHIP_H}" rx="6" fill="${layer.color}" fill-opacity="0.18" stroke="${layer.color}" stroke-opacity="0.55"/><text x="${cx + 10}" y="${cy + 17}" class="chip-text">${esc(label)}</text><title>${esc(c.fqn)}\n${esc(c.module)}${c.stereotype ? `\n@${esc(c.stereotype)}` : ''}</title></g>`;
        })
        .join('');

      const empty =
        total === 0
          ? `<text x="${PAD_X + 22}" y="${y + 80}" class="empty">— keine Klassen in dieser Schicht —</text>`
          : '';

      return `${stripe}${accent}${head}${stereoBadges}${chips}${empty}`;
    })
    .join('');

  // Edges between bands. Stroke width scales with count, capped at 8px.
  const layerY = new Map(bands.map((b) => [b.layer.id, b.y + b.h / 2]));
  const layerColor = new Map(bands.map((b) => [b.layer.id, b.layer.color]));
  const maxEdge = Math.max(1, ...flow.edges.map((e) => e.count));
  const xCenter = W / 2;
  const edgeSvg = flow.edges
    .map((edge, i) => {
      const yFrom = layerY.get(edge.from);
      const yTo = layerY.get(edge.to);
      if (yFrom === undefined || yTo === undefined) return '';
      const width = Math.max(1.4, Math.min(8, (edge.count / maxEdge) * 7 + 1.4));
      // Curve sideways so multiple edges don't overlap.
      const offset = (i - flow.edges.length / 2) * 26;
      const xMid = xCenter + offset;
      const midY = (yFrom + yTo) / 2;
      const arrow = yFrom < yTo ? '▼' : '▲';
      const color = layerColor.get(edge.from) ?? '#9ca3af';
      const path = `M ${xCenter} ${yFrom} C ${xMid} ${(yFrom + midY) / 2}, ${xMid} ${(midY + yTo) / 2}, ${xCenter} ${yTo}`;
      return `<g class="edge"><path d="${path}" stroke="${color}" stroke-width="${width}" stroke-opacity="0.55" fill="none"/><text x="${xMid}" y="${midY + 4}" text-anchor="middle" class="edge-label">${arrow} ${edge.count}</text></g>`;
    })
    .join('');

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}">
      <style>
        text{fill:#dce3f0;font:13px ui-sans-serif,system-ui,sans-serif}
        .title{font-size:20px;font-weight:600}
        .subtitle{font-size:12px;fill:#94a3b8}
        .band-title{font-size:16px;font-weight:700}
        .band-desc{font-size:12px;fill:#94a3b8}
        .band-count{font-size:12px;font-weight:600}
        .stereo{font-size:10px;font-family:ui-monospace,monospace;fill:#cbd5e1}
        .chip{cursor:default}
        .chip-text{font-size:11px;font-family:ui-monospace,monospace}
        .empty{font-size:12px;fill:#64748b;font-style:italic}
        .edge-label{font-size:10px;fill:#cbd5e1;font-family:ui-monospace,monospace}
      </style>
      <rect width="100%" height="100%" fill="#090d14"/>
      <text x="${PAD_X}" y="40" class="title">Architektur-Schichten</text>
      <text x="${PAD_X}" y="60" class="subtitle">${flow.total_classes} Klassen · ${flow.total_modules} Module · ${flow.cross_module_edges} Cross-Module-Edges</text>
      ${edgeSvg}
      ${bandSvg}
    </svg>`;
}

function shortChipLabel(name: string): string {
  return name.length <= 16 ? name : `${name.slice(0, 15)}…`;
}
