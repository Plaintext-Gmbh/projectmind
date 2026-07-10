/// Module-chord (dependency-wheel) SVG renderer — extracted from
/// `DiagramView.svelte` (Viz-Katalog V1.3, #61). Modules become arcs on a
/// circle, cross-module edges become Bezier chords through the centre; chord
/// width encodes the relation count. Self-edges are not drawn (a module's
/// `internal` count is shown on its label instead). Pure.

import { esc } from './common';

export interface ChordModule {
  id: string;
  label: string;
  classes: number;
  outgoing: number;
  incoming: number;
  internal: number;
}
export interface ChordEdge {
  from: string;
  to: string;
  count: number;
}
export interface ModuleChord {
  root: string;
  modules: ChordModule[];
  edges: ChordEdge[];
  total_relations: number;
}

export function renderModuleChord(chord: ModuleChord): string {
  const W = 900;
  const H = 700;
  const cx = W / 2;
  const cy = H / 2 + 12;
  const R = 270;
  const innerR = R - 18;
  const labelR = R + 28;

  if (chord.modules.length === 0) {
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}">
        <rect width="100%" height="100%" fill="#090d14"/>
        <text x="${cx}" y="${cy}" text-anchor="middle" fill="#94a3b8" font-size="18" font-family="ui-sans-serif,system-ui">Keine Module erkannt.</text>
      </svg>`;
  }

  const PALETTE = ['#3b82f6', '#10b981', '#f59e0b', '#ef4444', '#8b5cf6', '#ec4899', '#14b8a6', '#f97316', '#22d3ee', '#a3e635', '#eab308', '#6366f1'];

  // Position each module on the rim with weight proportional to its
  // class count (so big modules get a wider arc).
  const totalWeight = chord.modules.reduce((s, m) => s + Math.max(1, m.classes), 0);
  const arcs = new Map<string, { start: number; end: number; mid: number; color: string }>();
  let theta = -Math.PI / 2; // start at top
  chord.modules.forEach((m, i) => {
    const w = Math.max(1, m.classes) / totalWeight;
    const span = w * 2 * Math.PI * 0.94; // leave 6% gap total
    const start = theta + 0.03 * (2 * Math.PI) / chord.modules.length;
    const end = start + span;
    arcs.set(m.id, { start, end, mid: (start + end) / 2, color: PALETTE[i % PALETTE.length] });
    theta = end + 0.03 * (2 * Math.PI) / chord.modules.length;
  });

  const arcSvg = chord.modules
    .map((m) => {
      const a = arcs.get(m.id)!;
      const x1 = cx + R * Math.cos(a.start);
      const y1 = cy + R * Math.sin(a.start);
      const x2 = cx + R * Math.cos(a.end);
      const y2 = cy + R * Math.sin(a.end);
      const x3 = cx + innerR * Math.cos(a.end);
      const y3 = cy + innerR * Math.sin(a.end);
      const x4 = cx + innerR * Math.cos(a.start);
      const y4 = cy + innerR * Math.sin(a.start);
      const large = a.end - a.start > Math.PI ? 1 : 0;
      const path = `M ${x1} ${y1} A ${R} ${R} 0 ${large} 1 ${x2} ${y2} L ${x3} ${y3} A ${innerR} ${innerR} 0 ${large} 0 ${x4} ${y4} Z`;
      const lx = cx + labelR * Math.cos(a.mid);
      const ly = cy + labelR * Math.sin(a.mid);
      const anchor = Math.cos(a.mid) > 0 ? 'start' : 'end';
      const rotation = (a.mid * 180) / Math.PI;
      const niceRot = rotation > 90 || rotation < -90 ? rotation + 180 : rotation;
      const labelText = `${esc(m.label)} · ${m.classes}`;
      return `<g class="rim"><path d="${path}" fill="${a.color}" fill-opacity="0.85" stroke="${a.color}" stroke-width="1"/><text x="${lx}" y="${ly}" text-anchor="${anchor}" transform="rotate(${niceRot.toFixed(1)} ${lx} ${ly})" class="rim-label">${labelText}</text><title>${esc(m.id)}\n${m.classes} Klassen · ↗ ${m.outgoing} · ↘ ${m.incoming} · ⤾ ${m.internal}</title></g>`;
    })
    .join('');

  const maxEdge = Math.max(1, ...chord.edges.map((e) => e.count));
  const chordSvg = chord.edges
    .map((edge) => {
      const a1 = arcs.get(edge.from);
      const a2 = arcs.get(edge.to);
      if (!a1 || !a2) return '';
      // Tap each chord into a sub-portion of the arc proportional to
      // edge weight relative to the module's total outgoing.
      const x1 = cx + innerR * Math.cos(a1.mid);
      const y1 = cy + innerR * Math.sin(a1.mid);
      const x2 = cx + innerR * Math.cos(a2.mid);
      const y2 = cy + innerR * Math.sin(a2.mid);
      const w = Math.max(1, Math.min(6, (edge.count / maxEdge) * 5 + 1));
      return `<path d="M ${x1} ${y1} Q ${cx} ${cy} ${x2} ${y2}" stroke="${a1.color}" stroke-width="${w}" stroke-opacity="0.45" fill="none"><title>${esc(edge.from)} → ${esc(edge.to)}: ${edge.count}</title></path>`;
    })
    .join('');

  const top = chord.edges.slice(0, 6);
  const topList = top
    .map(
      (e, i) =>
        `<text x="24" y="${600 + i * 16}" class="top-line">${i + 1}. ${esc(e.from)} → ${esc(e.to)} · ${e.count}</text>`,
    )
    .join('');

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}">
      <style>
        text{fill:#dce3f0;font:13px ui-sans-serif,system-ui,sans-serif}
        .title{font-size:20px;font-weight:600}
        .subtitle{font-size:12px;fill:#94a3b8}
        .rim-label{font-size:11px;font-family:ui-monospace,monospace}
        .top-title{font-size:12px;font-weight:600;fill:#cbd5e1}
        .top-line{font-size:11px;font-family:ui-monospace,monospace;fill:#94a3b8}
      </style>
      <rect width="100%" height="100%" fill="#090d14"/>
      <text x="24" y="34" class="title">Modul-Kopplung</text>
      <text x="24" y="52" class="subtitle">${chord.modules.length} Module · ${chord.edges.length} Cross-Module-Edges · ${chord.total_relations} Beziehungen gesamt</text>
      ${chordSvg}
      ${arcSvg}
      ${top.length > 0 ? `<text x="24" y="584" class="top-title">Top Cross-Module-Kanten</text>${topList}` : ''}
    </svg>`;
}
