/// HSL fill colours for the folder map's git-driven overlays.
///
/// Lives in its own file so the maths can be unit-tested without dragging
/// the whole `DiagramView` component into the test runner. The component
/// imports both functions and the identity helper.

/// Map a "seconds since last commit" value onto an HSL colour. Brand-new
/// edits land in hot orange (~hue 18°); a year-old file decays into cool
/// grey-blue (~hue 220°). Saturation drops with age so old code recedes
/// visually. Log scale because the interesting structure lives in the
/// last few days vs. the long tail of stale files.
///
/// `secs_ago` is clamped to a minimum of 60 s so commits with future
/// timestamps and brand-new commits both anchor at the hot end of the
/// scale. Negative inputs collapse to that same floor.
export function recencyColor(secs_ago: number): string {
  const day = 86_400;
  const safe = Math.max(secs_ago, 60);
  // t=0 at <1 day, t≈0.33 at ~10 days, t≈0.67 at ~year, t≥1 at 1000+ days.
  const t = Math.min(1, Math.max(0, Math.log10(safe / day) / 3));
  const hue = 18 + (220 - 18) * t;
  const sat = 78 - 50 * t;
  const light = 52 - 18 * t;
  return `hsl(${hue.toFixed(0)}, ${sat.toFixed(0)}%, ${light.toFixed(0)}%)`;
}

/// Map an author identity (email when available, else display name) onto
/// a stable HSL colour. djb2-style 32-bit hash → hue; saturation and
/// lightness are fixed so all authors render at the same chroma. This
/// is intentionally not "primary author by line count" — that would
/// require git blame and far more work; "most recent committer per file"
/// is a cheap proxy that correlates well in practice.
export function authorColor(identity: string): string {
  let h = 5381;
  for (let i = 0; i < identity.length; i++) {
    h = ((h << 5) + h + identity.charCodeAt(i)) | 0;
  }
  // Use the unsigned 32-bit mod 360 so identical strings always map to
  // the same hue across reloads / processes.
  const hue = (h >>> 0) % 360;
  return `hsl(${hue}, 60%, 52%)`;
}

/// Pick the stablest per-author identity from a name + email pair. Email
/// wins when present (people change display names but not email); falls
/// back to the display name; null when the signature was empty.
export function authorIdentity(
  name: string | null | undefined,
  email: string | null | undefined,
): string | null {
  if (email && email.trim()) return email.trim().toLowerCase();
  if (name && name.trim()) return name.trim();
  return null;
}

/// One swatch in the recency legend: the sampled age (seconds) and the
/// colour `recencyColor` produces for it. Kept in this module so the legend
/// swatches can never drift from the scale that colours the boxes.
export interface RecencyLegendStop {
  /// i18n key for the label ("today" · "this week" · ">6 months").
  key: 'today' | 'week' | 'stale';
  /// Representative age in seconds used to sample the colour.
  secs_ago: number;
  /// The `recencyColor` swatch for that age.
  color: string;
}

const DAY = 86_400;

/// Fixed three-stop legend for the recency heatmap (#63.1): today, this
/// week, and the >6-month stale tail. Sampling the actual `recencyColor`
/// keeps the swatches honest — change the scale and the legend follows.
export function recencyLegend(): RecencyLegendStop[] {
  return [
    { key: 'today', secs_ago: DAY / 6, color: recencyColor(DAY / 6) },
    { key: 'week', secs_ago: DAY * 3, color: recencyColor(DAY * 3) },
    { key: 'stale', secs_ago: DAY * 210, color: recencyColor(DAY * 210) },
  ];
}

/// Per-file git fact the author legend aggregates over: who last touched it
/// and how long ago. Mirrors the component's per-path cache, but as a plain
/// value so the aggregation stays pure and unit-testable.
export interface AuthorFact {
  /// Stable author identity (see {@link authorIdentity}); null skips the file.
  author: string | null;
  /// Seconds since that author's most-recent touching commit.
  secs_ago: number;
}

/// One row in the author overlay's side legend (#63.2). `commits` counts the
/// files where this author is the most-recent toucher (the cheap proxy the
/// overlay already uses); `lastTouchedSecsAgo` is the freshest of those.
export interface AuthorLegendRow {
  identity: string;
  color: string;
  /// Files most-recently touched by this author.
  commits: number;
  /// Smallest `secs_ago` across this author's files.
  lastTouchedSecsAgo: number;
}

/// Aggregate per-file author facts into the legend rows. Deterministic: rows
/// are sorted by commit count descending, then identity ascending, so the
/// same repo always renders the same legend order. Files with no author are
/// skipped. An `undefined`/empty input yields an empty legend (no throw).
export function buildAuthorLegend(facts: Iterable<AuthorFact>): AuthorLegendRow[] {
  const byAuthor = new Map<string, { commits: number; lastTouchedSecsAgo: number }>();
  for (const f of facts) {
    if (!f || !f.author) continue;
    const cur = byAuthor.get(f.author);
    if (cur) {
      cur.commits += 1;
      if (f.secs_ago < cur.lastTouchedSecsAgo) cur.lastTouchedSecsAgo = f.secs_ago;
    } else {
      byAuthor.set(f.author, { commits: 1, lastTouchedSecsAgo: f.secs_ago });
    }
  }
  return [...byAuthor.entries()]
    .map(([identity, agg]) => ({
      identity,
      color: authorColor(identity),
      commits: agg.commits,
      lastTouchedSecsAgo: agg.lastTouchedSecsAgo,
    }))
    .sort((a, b) => b.commits - a.commits || a.identity.localeCompare(b.identity));
}

/// Compact relative-age label for hover tooltips ("just now", "3d", "5w",
/// "2y"). Coarse by design — the legend hover wants a glance-value, not a
/// precise timestamp. Negative / sub-minute ages collapse to "just now".
export function humanizeAge(secs_ago: number): string {
  if (secs_ago < 60) return 'just now';
  const min = Math.floor(secs_ago / 60);
  if (min < 60) return `${min}m`;
  const hours = Math.floor(secs_ago / 3600);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(secs_ago / DAY);
  if (days < 7) return `${days}d`;
  const weeks = Math.floor(days / 7);
  if (days < 30) return `${weeks}w`;
  const months = Math.floor(days / 30);
  if (days < 365) return `${months}mo`;
  const years = Math.floor(days / 365);
  return `${years}y`;
}
