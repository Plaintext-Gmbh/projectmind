<script lang="ts">
  import { tick } from 'svelte';
  import { showDiff, type DiffFocus } from '../lib/api';
  import { t } from '../lib/i18n';
  import { createShiftWheelZoom } from '../lib/shiftWheelZoom';
  import { buildDiffIndex, focusLineIndex, type DiffFile, type DiffLine } from '../lib/diffFocus';
  import DiffRail from './DiffRail.svelte';

  export let reference: string;
  export let to: string | null = null;
  /// Optional focus inside the diff (#126). When set, the matching
  /// hunk / line is scrolled into view and pulsed once. Tour steps
  /// without a focus pass `null`/`undefined` and the diff renders
  /// exactly like before.
  export let focus: DiffFocus | null = null;

  let raw = '';
  let lines: DiffLine[] = [];
  let loading = false;
  let error: string | null = null;

  /// Structured per-file / per-hunk navigation index (#126). Drives the
  /// side rail; derived from `lines` so the flat render stays the source
  /// of truth for the visual diff.
  let diffFiles: DiffFile[] = [];

  /// `index → element ref` map of rendered diff lines. Populated by
  /// the `bind:this` on the `<span class="line">` block; used to
  /// scroll the focused line into view.
  let lineEls: HTMLSpanElement[] = [];
  /// Index of the line that should currently pulse, or `null` for no
  /// pulse. Cleared after the CSS animation finishes so the same
  /// focus can be re-triggered when the tour pointer moves.
  let pulseIdx: number | null = null;
  /// Flat-line index the diff is currently focused on (tour focus or a
  /// rail click). Drives the rail's active marker. Persists past the
  /// pulse so the rail keeps highlighting the last-visited hunk.
  let activeLine: number | null = null;

  // Shift + wheel zoom, persisted under the per-component key.
  const { zoom, action: zoomAction } = createShiftWheelZoom('projectmind.diffview.zoom');

  $: if (reference) void load(reference, to);
  /// React to focus changes after the load — the user can scrub through
  /// tour steps that share `reference`/`to` but tweak the focus.
  $: if (lines.length > 0 && focus !== undefined) void applyFocus(focus);

  async function load(ref: string, target: string | null) {
    loading = true;
    error = null;
    try {
      raw = await showDiff(ref, target ?? undefined);
      lines = parse(raw);
      diffFiles = buildDiffIndex(lines);
      lineEls = [];
      activeLine = null;
      // Wait for the bind:this refs to land, then react to the current focus.
      await tick();
      await applyFocus(focus);
    } catch (err) {
      error = String(err);
      lines = [];
      diffFiles = [];
    } finally {
      loading = false;
    }
  }

  async function applyFocus(f: DiffFocus | null | undefined) {
    if (!f || lines.length === 0) {
      pulseIdx = null;
      return;
    }
    const idx = focusLineIndex(lines, f);
    if (idx === null) {
      pulseIdx = null;
      return;
    }
    await scrollAndPulse(idx);
  }

  /// Scroll the flat-line at `idx` into view, mark it active for the rail,
  /// and play a one-shot pulse. Shared by tour focus and rail clicks so
  /// both go through exactly one navigation path (#126).
  async function scrollAndPulse(idx: number) {
    await tick();
    const el = lineEls[idx];
    if (!el) return;
    el.scrollIntoView({ block: 'center', behavior: 'smooth' });
    activeLine = idx;
    pulseIdx = idx;
    // Clear the pulse after the animation duration so re-focusing the
    // same line later still triggers the highlight. `activeLine` stays
    // so the rail keeps the marker highlighted.
    window.setTimeout(() => {
      if (pulseIdx === idx) pulseIdx = null;
    }, 1400);
  }

  function onRailJump(e: CustomEvent<{ startLine: number }>) {
    void scrollAndPulse(e.detail.startLine);
  }

  function parse(diff: string): DiffLine[] {
    return diff.split('\n').map((text) => {
      if (text.startsWith('diff --git ')) return { kind: 'header' as const, text };
      if (
        text.startsWith('--- ') ||
        text.startsWith('+++ ') ||
        text.startsWith('index ') ||
        text.startsWith('similarity index ') ||
        text.startsWith('rename from ') ||
        text.startsWith('rename to ') ||
        text.startsWith('new file ') ||
        text.startsWith('deleted file ')
      )
        return { kind: 'meta' as const, text };
      if (text.startsWith('@@')) return { kind: 'hunk' as const, text };
      if (text.startsWith('+')) return { kind: 'add' as const, text };
      if (text.startsWith('-')) return { kind: 'del' as const, text };
      return { kind: 'context' as const, text };
    });
  }
</script>

<section class="root" use:zoomAction style="font-size: {$zoom}em;">
  <header class="bar">
    <span class="kind">{$t('diff.kind')}</span>
    <code class="ref">{reference}</code>
    <span class="arrow">→</span>
    <code class="ref">{to ?? $t('diff.workingTree')}</code>
  </header>
  {#if loading}
    <div class="status">{$t('diff.computing')}</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
  {:else if lines.length === 0}
    <div class="status">{$t('diff.noChanges')}</div>
  {:else}
    <div class="body">
      <pre class="diff"><!--
   --><!-- prettier-ignore -->{#each lines as l, i (i)}<span class="line {l.kind}" class:pulse={pulseIdx === i} bind:this={lineEls[i]}>{l.text || ' '}</span>
{/each}</pre>
      <DiffRail files={diffFiles} {activeLine} on:jump={onRailJump} />
    </div>
  {/if}
</section>

<style>
  .root {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 16px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--bg-3);
    font-size: 0.86em;
    color: var(--fg-1);
  }
  .kind {
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 2px 6px;
    background: var(--bg-2);
    border-radius: 3px;
    color: var(--fg-2);
    font-size: 0.72em;
  }
  .arrow {
    color: var(--fg-2);
  }
  .ref {
    font-family: var(--mono);
    color: var(--fg-0);
    background: var(--bg-2);
    padding: 1px 6px;
    border-radius: 3px;
  }

  .status,
  .error {
    padding: 24px;
    color: var(--fg-2);
  }
  .error {
    color: var(--error);
  }

  /* Diff area + navigation rail sit side-by-side and share the flex row.
     The theme-aware diff colours live here so both the diff body and the
     embedded rail (#126) inherit them. */
  .body {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
    /* Defaults below are for the dark theme; light theme overrides follow
       the matching :global(:root[data-theme='light']) block. */
    --diff-add-fg: #b8eaa6;
    --diff-add-bg: #2ea043;
    --diff-del-fg: #f8b6b6;
    --diff-del-bg: #cf222e;
  }

  :global(:root[data-theme='light']) .body {
    --diff-add-fg: #044317;
    --diff-add-bg: #1a7f37;
    --diff-del-fg: #82071e;
    --diff-del-bg: #cf222e;
  }

  .diff {
    margin: 0;
    padding: 16px;
    font-family: var(--mono);
    font-size: 1em;
    line-height: 1.5;
    overflow: auto;
    flex: 1;
    min-width: 0;
    background: var(--bg-0);
    color: var(--fg-0);
    white-space: pre;
  }

  .line {
    display: block;
    padding: 0 12px;
    border-left: 3px solid transparent;
  }
  .line.header {
    color: var(--accent-2);
    font-weight: 600;
    margin-top: 1em;
    background: var(--bg-1);
    border-left-color: var(--accent-2);
  }
  .line.meta {
    color: var(--fg-2);
  }
  .line.hunk {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border-left-color: var(--accent);
    font-weight: 500;
  }
  .line.add {
    color: var(--diff-add-fg);
    background: color-mix(in srgb, var(--diff-add-bg) 22%, transparent);
    border-left-color: var(--diff-add-bg);
  }
  .line.del {
    color: var(--diff-del-fg);
    background: color-mix(in srgb, var(--diff-del-bg) 22%, transparent);
    border-left-color: var(--diff-del-bg);
  }
  /* Tour focus pulse (#126). Plays once when the focused line scrolls
     into view; cleared after 1.4s so the same line can be re-pulsed
     when the tour pointer moves. */
  .line.pulse {
    animation: diff-pulse 1.4s ease-out;
    border-left-color: var(--accent-2);
  }
  @keyframes diff-pulse {
    0% {
      background-color: color-mix(in srgb, var(--accent-2) 50%, transparent);
      box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent-2) 40%, transparent);
    }
    100% {
      background-color: transparent;
      box-shadow: none;
    }
  }
</style>
