/// Shared, dependency-free building blocks for the hand-rolled SVG diagram
/// renderers extracted out of `DiagramView.svelte` (Viz-Katalog V1.3, #61).
///
/// Every renderer under `app/src/lib/diagrams/<kind>.ts` is a pure function
/// `render(payload, opts): string` that returns an SVG string — no DOM, no
/// Svelte, so the layout maths can be unit-tested in isolation the way
/// `treemap.ts` already is. This module holds the primitives those renderers
/// share (HTML escaping, byte formatting, label truncation).

/// Escape the five XML-significant characters so untrusted repo strings
/// (file names, author names, class fqns) are safe to inline into an SVG
/// string. Kept byte-for-byte identical to the previous in-component `esc`
/// so the extraction stays behaviour-preserving.
export function esc(s: string): string {
  return s.replace(/[&<>"']/g, (ch) => {
    const map: Record<string, string> = {
      '&': '&amp;',
      '<': '&lt;',
      '>': '&gt;',
      '"': '&quot;',
      "'": '&#39;',
    };
    return map[ch] ?? ch;
  });
}

/// Human-readable byte size (B / KB / MB / GB). Used by the language-stats
/// renderer for the per-bucket + total size read-outs.
export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/// Truncate a label to `max` characters, appending an ellipsis when clipped.
export function shortLabel(label: string, max: number): string {
  return label.length <= max ? label : `${label.slice(0, max - 1)}…`;
}
