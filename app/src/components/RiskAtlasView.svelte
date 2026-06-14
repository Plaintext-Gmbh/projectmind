<script lang="ts">
  import { onMount } from 'svelte';
  import { riskAtlas, type RiskScore, type RiskWeights } from '../lib/api';
  import { classes, selectedClass, viewMode } from '../lib/store';
  import { get } from 'svelte/store';

  // Cockpit 2.1 — Risk Atlas (Issue #157):
  //   Treemap (Fläche = SLOC, Farbe = Score) auf dem bereits gemergten risk_atlas-Backend.
  //   Klick auf eine Kachel öffnet die Klasse im Code-Tab; Gewichts-Slider re-querien das
  //   Backend und werden in localStorage persistiert.

  const WEIGHTS_KEY = 'projectmind.risk.weights';
  const DEFAULT_WEIGHTS: RiskWeights = { churn: 1, cx: 1, cov: 0, deps: 0 };
  const WEIGHT_FIELDS: [keyof RiskWeights, string][] = [
    ['churn', 'Churn'],
    ['cx', 'Complexity'],
    ['cov', 'Coverage'],
    ['deps', 'Deps'],
  ];

  let weights: RiskWeights = loadWeights();
  let scores: RiskScore[] = [];
  let windowDays = 90;
  let loading = false;
  let error: string | null = null;

  // Container-Pixelmasse (für das Treemap-Layout).
  let boxW = 0;
  let boxH = 0;

  interface Rect {
    x: number;
    y: number;
    w: number;
    h: number;
    item: RiskScore;
  }

  function loadWeights(): RiskWeights {
    try {
      const raw = localStorage.getItem(WEIGHTS_KEY);
      if (raw) return { ...DEFAULT_WEIGHTS, ...JSON.parse(raw) };
    } catch {
      /* localStorage nicht verfügbar -> Defaults */
    }
    return { ...DEFAULT_WEIGHTS };
  }

  function persistWeights() {
    try {
      localStorage.setItem(WEIGHTS_KEY, JSON.stringify(weights));
    } catch {
      /* egal */
    }
  }

  async function load() {
    loading = true;
    error = null;
    try {
      const res = await riskAtlas({ top: 250, weights });
      scores = res.scores;
      windowDays = res.window_days;
    } catch (err) {
      error = String(err);
      scores = [];
    } finally {
      loading = false;
    }
  }

  // Slider-Änderung: persistieren + Backend neu abfragen (debounced).
  let reloadTimer: ReturnType<typeof setTimeout> | undefined;
  function onWeightChange() {
    persistWeights();
    clearTimeout(reloadTimer);
    reloadTimer = setTimeout(load, 250);
  }

  function resetWeights() {
    weights = { ...DEFAULT_WEIGHTS };
    onWeightChange();
  }

  // Rekursiver, flächentreuer Treemap (alternierende Halbierung nach kumuliertem SLOC).
  // Bewusst simpel + korrekt; squarified-Optimierung der Seitenverhältnisse ist ein Follow-up.
  function val(it: RiskScore): number {
    return Math.max(it.sloc, 1);
  }

  function treemap(items: RiskScore[], x: number, y: number, w: number, h: number): Rect[] {
    if (items.length === 0 || w <= 0 || h <= 0) return [];
    if (items.length === 1) return [{ x, y, w, h, item: items[0] }];
    const total = items.reduce((s, it) => s + val(it), 0);
    let acc = 0;
    let split = 1;
    for (let i = 0; i < items.length - 1; i++) {
      acc += val(items[i]);
      if (acc >= total / 2) {
        split = i + 1;
        break;
      }
    }
    const a = items.slice(0, split);
    const b = items.slice(split);
    const frac = a.reduce((s, it) => s + val(it), 0) / total;
    if (w >= h) {
      const aw = w * frac;
      return [...treemap(a, x, y, aw, h), ...treemap(b, x + aw, y, w - aw, h)];
    }
    const ah = h * frac;
    return [...treemap(a, x, y, w, ah), ...treemap(b, x, y + ah, w, h - ah)];
  }

  // Farbskala: Score 0 (grün) -> 100 (rot).
  function colorFor(score: number): string {
    const s = Math.max(0, Math.min(100, score));
    const hue = 120 - (s / 100) * 120;
    return `hsl(${hue}, 65%, 42%)`;
  }

  function shortName(fqn: string): string {
    const i = fqn.lastIndexOf('.');
    return i >= 0 ? fqn.slice(i + 1) : fqn;
  }

  // Klick: passende ClassEntry im Store finden und im Code-Tab öffnen (bestehende Mechanik).
  function openClass(fqn: string) {
    const match = get(classes).find((c) => c.fqn === fqn);
    if (match) {
      selectedClass.set(match);
      viewMode.set('classes');
    }
  }

  $: rects = boxW > 0 && boxH > 0 && scores.length > 0 ? treemap([...scores].sort((p, q) => val(q) - val(p)), 0, 0, boxW, boxH) : [];

  onMount(load);
</script>

<div class="risk-atlas">
  <header class="risk-bar">
    <div class="risk-title">
      <strong>Risk Atlas</strong>
      <span class="muted">Fläche = SLOC · Farbe = Score · Fenster {windowDays} Tage · {scores.length} Klassen</span>
    </div>
    <div class="weights">
      {#each WEIGHT_FIELDS as [key, label]}
        <label class="weight">
          <span>{label}</span>
          <input
            type="range"
            min="0"
            max="2"
            step="0.1"
            bind:value={weights[key]}
            on:input={onWeightChange}
          />
          <span class="wval">{weights[key].toFixed(1)}</span>
        </label>
      {/each}
      <button class="reset" on:click={resetWeights} title="Gewichte zurücksetzen">↺</button>
    </div>
  </header>

  {#if error}
    <div class="risk-msg error">{error}</div>
  {:else if loading && scores.length === 0}
    <div class="risk-msg">Lade Risk Atlas …</div>
  {:else if scores.length === 0}
    <div class="risk-msg">Keine Daten (kein Repo offen oder keine Klassen).</div>
  {:else}
    <div class="treemap" bind:clientWidth={boxW} bind:clientHeight={boxH}>
      {#each rects as r (r.item.fqn)}
        <button
          class="tile"
          style="left:{r.x}px; top:{r.y}px; width:{r.w}px; height:{r.h}px; background:{colorFor(r.item.score)};"
          title={`${r.item.fqn}\nScore ${r.item.score.toFixed(0)} · Churn ${r.item.churn} · CX ${r.item.cx} · SLOC ${r.item.sloc}\n${r.item.why}`}
          on:click={() => openClass(r.item.fqn)}
        >
          {#if r.w > 46 && r.h > 22}
            <span class="tile-label">{shortName(r.item.fqn)}</span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .risk-atlas {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .risk-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 10px;
    flex-wrap: wrap;
    border-bottom: 1px solid var(--border, #2a2a2a);
  }
  .risk-title strong {
    margin-right: 8px;
  }
  .muted {
    color: var(--muted, #888);
    font-size: 12px;
  }
  .weights {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }
  .weight {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
  }
  .weight input[type='range'] {
    width: 80px;
  }
  .wval {
    width: 22px;
    text-align: right;
    font-variant-numeric: tabular-nums;
    color: var(--muted, #888);
  }
  .reset {
    cursor: pointer;
  }
  .risk-msg {
    padding: 16px;
    color: var(--muted, #888);
  }
  .risk-msg.error {
    color: #e74c3c;
    white-space: pre-wrap;
  }
  .treemap {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .tile {
    position: absolute;
    border: 1px solid rgba(0, 0, 0, 0.35);
    margin: 0;
    padding: 2px 4px;
    overflow: hidden;
    cursor: pointer;
    color: #fff;
    text-align: left;
    font: inherit;
    display: block;
  }
  .tile:hover {
    outline: 2px solid #fff;
    outline-offset: -2px;
    z-index: 1;
  }
  .tile-label {
    font-size: 11px;
    line-height: 1.1;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6);
    word-break: break-word;
  }
</style>
