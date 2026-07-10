<script lang="ts">
  // Walkthrough `diagram-diff` step (#125).
  //
  // Shows a before/after architecture snapshot for one diagram kind. The
  // first implementation supports the folder map: `show_diagram('folder-map')`
  // renders the current tree, and `list_changes_since(from, to)` supplies the
  // file-level change set overlaid on it. The user toggles between three
  // static modes — before / after / changed-only. Changed leaf nodes pulse
  // once when the step opens; unchanged nodes fade back.
  //
  // The heavy lifting (changed-node derivation, mode filtering, SVG) lives in
  // `lib/diagramDiff.ts` so it is unit-tested without a DOM. This component is
  // the thin shell that loads the data, owns the toggle + pulse lifecycle, and
  // reuses the same shift-wheel zoom action as the other diagram views.
  import { onMount, tick } from 'svelte';
  import { showDiagram, listChangesSince } from '../../lib/api';
  import { t } from '../../lib/i18n';
  import { createShiftWheelZoom } from '../../lib/shiftWheelZoom';
  import {
    deriveChangedNodes,
    filterNodesForMode,
    changedPulseIds,
    changedFileCount,
    renderFolderDiff,
    diffStatusGlyph,
    DIAGRAM_DIFF_MODES,
    type FolderMap,
    type DiagramDiffMode,
    type DiffStatus,
  } from '../../lib/diagramDiff';

  export let diagram: 'folder-map' = 'folder-map';
  export let from: string;
  export let to: string | null = null;

  // Shift-wheel zoom, shared action + store used by the other diagram views.
  const { zoom, action: zoomAction } = createShiftWheelZoom(
    'projectmind.walkthrough.diagramDiff.zoom',
  );

  let map: FolderMap | null = null;
  let statusById: Map<string, DiffStatus> = new Map();
  let loading = true;
  let error: string | null = null;

  /// Persist the last-used mode so flipping steps keeps the reader's choice.
  const MODE_KEY = 'projectmind.walkthrough.diagramDiff.mode';
  let mode: DiagramDiffMode = readMode();

  /// Leaves currently marked to pulse. Populated when the step's data lands
  /// (and on re-entry), then cleared after the CSS animation so the same
  /// nodes can pulse again next time the step opens.
  let pulseIds = new Set<string>();
  let pulseTimer: ReturnType<typeof setTimeout> | null = null;

  function readMode(): DiagramDiffMode {
    try {
      const v = localStorage.getItem(MODE_KEY);
      if (v === 'before' || v === 'after' || v === 'changed-only') return v;
    } catch {
      // localStorage unavailable
    }
    return 'after';
  }

  function setMode(v: DiagramDiffMode) {
    if (mode === v) return;
    mode = v;
    try {
      localStorage.setItem(MODE_KEY, v);
    } catch {
      // ignore
    }
    schedulePulse();
  }

  $: changedCount = map ? changedFileCount(map, statusById) : 0;

  // Recompute the SVG whenever the map, overlay, mode or pulse set changes.
  $: svg = map ? renderFolderDiff(map, statusById, { mode, pulseIds }) : '';

  /// Fire the one-shot pulse for the currently visible changed leaves. Only
  /// nodes the active mode actually renders are pulsed (changed-only filters
  /// most away). Cleared after the animation window.
  function schedulePulse() {
    if (!map) return;
    const visible = new Set(filterNodesForMode(map, statusById, mode).map((n) => n.id));
    const ids = changedPulseIds(map, statusById, visible);
    if (pulseTimer) clearTimeout(pulseTimer);
    if (ids.length === 0) {
      pulseIds = new Set();
      return;
    }
    pulseIds = new Set(ids);
    pulseTimer = setTimeout(() => {
      pulseIds = new Set();
    }, 1500);
  }

  async function load() {
    loading = true;
    error = null;
    try {
      const [payload, delta] = await Promise.all([
        showDiagram(diagram),
        listChangesSince(from, to ?? undefined),
      ]);
      map = JSON.parse(payload) as FolderMap;
      statusById = deriveChangedNodes(map, delta);
      await tick();
      schedulePulse();
    } catch (err) {
      error = String(err);
      map = null;
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void load();
    return () => {
      if (pulseTimer) clearTimeout(pulseTimer);
    };
  });

  $: rangeLabel = to ? `${from} → ${to}` : `${from} → ${$t('walkthrough.diagramDiff.workingTree')}`;

  // Legend rows shown when there's a change overlay to explain.
  const LEGEND: Array<{ status: DiffStatus; key: string }> = [
    { status: 'added', key: 'walkthrough.diagramDiff.legend.added' },
    { status: 'modified', key: 'walkthrough.diagramDiff.legend.modified' },
    { status: 'deleted', key: 'walkthrough.diagramDiff.legend.deleted' },
    { status: 'renamed', key: 'walkthrough.diagramDiff.legend.renamed' },
  ];
</script>

<div class="dd-step">
  <header class="dd-head">
    <div class="dd-title">
      <span class="dd-kicker">{$t('walkthrough.diagramDiff.kicker')}</span>
      <strong class="dd-diagram">{diagram}</strong>
      <span class="dd-range" title={rangeLabel}>{rangeLabel}</span>
      {#if !loading && !error}
        <span class="dd-count">{$t('walkthrough.diagramDiff.changedCount', { count: changedCount })}</span>
      {/if}
    </div>
    <div
      class="dd-modes"
      role="group"
      aria-label={$t('walkthrough.diagramDiff.modes.aria')}
    >
      {#each DIAGRAM_DIFF_MODES as m (m)}
        <button
          type="button"
          class="dd-mode"
          class:active={mode === m}
          aria-pressed={mode === m}
          on:click={() => setMode(m)}
        >
          {$t(`walkthrough.diagramDiff.mode.${m === 'changed-only' ? 'changedOnly' : m}`)}
        </button>
      {/each}
    </div>
  </header>

  {#if loading}
    <div class="dd-msg">…</div>
  {:else if error}
    <div class="dd-msg error">⚠ {error}</div>
  {:else if map}
    {#if mode !== 'before' && changedCount === 0}
      <div class="dd-empty">{$t('walkthrough.diagramDiff.noChanges')}</div>
    {/if}
    <div class="dd-stage" use:zoomAction>
      <div class="dd-canvas" style="transform: scale({$zoom}); transform-origin: top center;">
        <!-- eslint-disable-next-line svelte/no-at-html-tags -->
        {@html svg}
      </div>
    </div>
    {#if mode !== 'before' && changedCount > 0}
      <ul class="dd-legend" aria-label={$t('walkthrough.diagramDiff.legend.aria')}>
        {#each LEGEND as row (row.status)}
          <li>
            <span class="dd-swatch status-{row.status}" aria-hidden="true">{diffStatusGlyph(row.status)}</span>
            {$t(row.key)}
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</div>

<style>
  .dd-step {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }
  .dd-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    flex-wrap: wrap;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--bg-3, #2a2a2a);
    background: var(--bg-1, #1b1b1b);
    flex-shrink: 0;
  }
  .dd-title {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    flex-wrap: wrap;
    min-width: 0;
  }
  .dd-kicker {
    text-transform: uppercase;
    font-size: 0.7rem;
    letter-spacing: 0.08em;
    opacity: 0.7;
  }
  .dd-diagram {
    font-family: var(--mono, monospace);
    font-size: 0.9rem;
  }
  .dd-range {
    font-size: 0.8rem;
    opacity: 0.7;
    font-family: var(--mono, monospace);
  }
  .dd-count {
    font-size: 0.75rem;
    padding: 1px 8px;
    border-radius: 9px;
    background: var(--bg-2, #262626);
    color: var(--fg-1, #d0d0d0);
  }
  .dd-modes {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }
  .dd-mode {
    background: transparent;
    color: var(--fg-1, #d0d0d0);
    border: 1px solid transparent;
    border-radius: 4px;
    padding: 4px 12px;
    cursor: pointer;
    font: inherit;
    font-size: 0.85rem;
  }
  .dd-mode:hover {
    background: var(--bg-2, #262626);
  }
  .dd-mode.active {
    background: var(--bg-2, #262626);
    border-color: var(--accent, #d29922);
    color: var(--fg-0, #fff);
  }
  .dd-msg {
    padding: 1rem;
    color: var(--fg-2, #9aa8ba);
  }
  .dd-msg.error {
    color: var(--error, #e05252);
  }
  .dd-empty {
    padding: 0.4rem 0.75rem;
    font-size: 0.8rem;
    color: var(--fg-2, #9aa8ba);
    border-bottom: 1px solid var(--bg-3, #2a2a2a);
  }
  .dd-stage {
    flex: 1;
    min-height: 0;
    overflow: auto;
    background: #090d14;
  }
  .dd-canvas :global(svg) {
    display: block;
    width: 100%;
    height: auto;
  }
  .dd-legend {
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
    margin: 0;
    padding: 0.4rem 0.75rem;
    border-top: 1px solid var(--bg-3, #2a2a2a);
    background: var(--bg-1, #1b1b1b);
    font-size: 0.75rem;
    color: var(--fg-1, #d0d0d0);
    flex-shrink: 0;
  }
  .dd-legend li {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }
  .dd-swatch {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: 3px;
    font-family: var(--mono, monospace);
    font-size: 0.7rem;
    font-weight: 700;
    color: #fff;
  }
  .dd-swatch.status-added { background: #2ea043; }
  .dd-swatch.status-modified { background: #d29922; color: #1a1a1a; }
  .dd-swatch.status-deleted { background: #cf222e; }
  .dd-swatch.status-renamed { background: #8957e5; }
</style>
