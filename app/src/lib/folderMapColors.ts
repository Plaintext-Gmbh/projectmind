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
