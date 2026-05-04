/// Anchor sugar for walkthrough narration.
///
/// LLM-authored tours often want to reference other steps from inside the
/// markdown body — "as we saw in [step:2], …" or "[step:5|the bean graph
/// stop]". Rewriting that into a plain `pm:step:N` link before passing the
/// text to `marked` lets the existing pm-link click handler in
/// `WalkthroughView` do the rest, with no extra wiring.
///
/// Two forms supported, both 1-based (matches the human-visible "Step 3 of
/// 7" labels in the UI):
///
/// - `[step:3]` — short form, link text becomes `step 3`
/// - `[step:3|the bean graph stop]` — explicit label
///
/// Anything else (`[step:abc]`, missing closing bracket, …) is left
/// untouched so it can be reported to the human as a typo by the markdown
/// renderer rather than being silently rewritten.

const STEP_REF = /\[step:(\d+)(?:\|([^\]]*))?\]/g;

/// Rewrite `[step:N]` and `[step:N|label]` references inside `md` into
/// markdown links of the form `[label](pm:step:<N-1>)`.
///
/// The pm-link target is **0-based** (matches the `goTo(idx)` API the
/// click handler eventually calls) while the source notation is 1-based
/// for human readability — same convention used everywhere else in the
/// walkthrough UI ("Step 3 of 7").
export function expandStepRefs(md: string): string {
  return md.replace(STEP_REF, (_, num: string, label: string | undefined) => {
    const n = Number.parseInt(num, 10);
    if (!Number.isFinite(n) || n < 1) {
      // Out-of-range refs (`[step:0]`, `[step:-3]`) are left as plain
      // text so the author can spot the typo. We can't validate against
      // the actual step count here — that's a runtime concern of the
      // renderer.
      return `[step:${num}${label ? `|${label}` : ''}]`;
    }
    const text = (label?.trim() ?? '') || `step ${n}`;
    return `[${text}](pm:step:${n - 1})`;
  });
}

/// Same-file line anchor inside narration markdown. Matches `#L42`,
/// `#L42-58` and the `#L42-L58` / `#L42–L58` (en-dash) variants the LLM
/// occasionally emits.
const LINE_ANCHOR = /^#L(\d+)(?:[-–]L?(\d+))?$/;

/// Parse a same-file line anchor into a `{from, to}` line range. Returns
/// `null` for any other anchor (heading slugs, empty strings, …) so callers
/// can fall through to their default handler.
///
/// Single-line refs (`#L42`) collapse to `from === to` so the renderer can
/// flash one line. The 1-based line numbers match what the user sees in the
/// gutter.
export function matchLineAnchor(href: string): { from: number; to: number } | null {
  const m = LINE_ANCHOR.exec(href);
  if (!m) return null;
  const from = Number(m[1]);
  const to = m[2] ? Number(m[2]) : from;
  if (!Number.isFinite(from) || !Number.isFinite(to)) return null;
  // Reversed ranges (`#L20-10`) are normalised so the renderer doesn't
  // need to special-case them.
  if (to < from) return { from: to, to: from };
  return { from, to };
}
