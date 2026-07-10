<script lang="ts">
  // Walkthrough 2.0 `atlas` step (Cockpit 2.4, #160).
  //
  // Renders the Risk Atlas treemap (optionally scoped to one module) with the
  // step's `highlight_fqns` ringed as named hotspots. Reuses the shared
  // treemap layout so tiles match the standalone Risk Atlas view. Click jumps
  // to the class in the code tab.
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { riskAtlas, type RiskScore } from '../../lib/api';
  import { classes, selectedClass, viewMode } from '../../lib/store';
  import { treemap, tileValue, colorForScore, shortName, type TreemapRect } from '../../lib/treemap';

  export let module: string | null = null;
  export let highlightFqns: string[] = [];

  let scores: RiskScore[] = [];
  let loading = true;
  let error: string | null = null;
  let boxW = 0;
  let boxH = 0;

  $: highlightSet = new Set(highlightFqns);
  $: rects =
    boxW > 0 && boxH > 0 && scores.length > 0
      ? treemap(
          [...scores].sort((p, q) => tileValue(q) - tileValue(p)),
          0,
          0,
          boxW,
          boxH,
        )
      : [];
  // Named hotspots that actually resolved to a tile (for the legend).
  $: namedHotspots = rects.filter((r) => highlightSet.has(r.item.fqn));

  async function load() {
    loading = true;
    error = null;
    try {
      const res = await riskAtlas({ top: 250, module: module ?? undefined });
      scores = res.scores;
    } catch (err) {
      error = String(err);
      scores = [];
    } finally {
      loading = false;
    }
  }

  function openClass(fqn: string) {
    const match = get(classes).find((c) => c.fqn === fqn);
    if (match) {
      selectedClass.set(match);
      viewMode.set('classes');
    }
  }

  function tileTitle(r: TreemapRect): string {
    const cov = r.item.cov === null ? '—' : `${Math.round((r.item.cov ?? 0) * 100)}%`;
    return `${r.item.fqn}\nscore ${Math.round(r.item.score)} · churn ${r.item.churn} · cx ${r.item.cx} · cov ${cov}`;
  }

  onMount(load);
</script>

<div class="atlas-step">
  <header class="atlas-head">
    <span class="atlas-kicker">Atlas</span>
    <span class="atlas-scope">{module ? `module:${module}` : 'whole repo'}</span>
    {#if namedHotspots.length > 0}
      <span class="atlas-legend">◎ {namedHotspots.length} hotspot{namedHotspots.length === 1 ? '' : 's'}</span>
    {/if}
  </header>

  {#if loading}
    <div class="msg">…</div>
  {:else if error}
    <div class="msg error">⚠ {error}</div>
  {:else if scores.length === 0}
    <div class="msg">No risk data (no repo open or no scored classes).</div>
  {:else}
    <div class="atlas-canvas" bind:clientWidth={boxW} bind:clientHeight={boxH}>
      {#each rects as r (r.item.fqn)}
        {@const ringed = highlightSet.has(r.item.fqn)}
        <button
          class="tile"
          class:ringed
          style={`left:${r.x}px;top:${r.y}px;width:${r.w}px;height:${r.h}px;background:${colorForScore(r.item.score)}`}
          title={tileTitle(r)}
          on:click={() => openClass(r.item.fqn)}
        >
          {#if r.w > 46 && r.h > 16}
            <span class="tile-label">{shortName(r.item.fqn)}</span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .atlas-step {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .atlas-head {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--border, #2a2a2a);
    background: var(--panel, #1b1b1b);
  }
  .atlas-kicker {
    text-transform: uppercase;
    font-size: 0.7rem;
    letter-spacing: 0.08em;
    opacity: 0.7;
  }
  .atlas-scope {
    font-size: 0.8rem;
    opacity: 0.7;
    font-family: var(--mono, monospace);
  }
  .atlas-legend {
    font-size: 0.75rem;
    opacity: 0.8;
    margin-left: auto;
  }
  .msg {
    padding: 1rem;
  }
  .msg.error {
    color: #e05252;
  }
  .atlas-canvas {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    overflow: hidden;
  }
  .tile {
    position: absolute;
    border: 1px solid rgba(0, 0, 0, 0.35);
    padding: 1px 3px;
    margin: 0;
    overflow: hidden;
    cursor: pointer;
    color: #fff;
    text-align: left;
    font: inherit;
  }
  .tile-label {
    font-size: 0.68rem;
    text-shadow: 0 0 3px rgba(0, 0, 0, 0.7);
    white-space: nowrap;
  }
  .tile.ringed {
    outline: 3px solid #ffe08a;
    outline-offset: -3px;
    z-index: 2;
  }
</style>
