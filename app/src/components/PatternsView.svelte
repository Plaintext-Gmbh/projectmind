<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { patternCheck, PATTERN_LABELS, type PatternResult } from '../lib/api';
  import { buildHeatmap, type HeatmapCell } from '../lib/patterns';
  import { classes, selectedClass, viewMode } from '../lib/store';

  // Cockpit 2.3 — Pattern Lens (Issue #159):
  //   Compliance-Heatmap (Zeilen = Muster, Spalten = Module, Zellen = ✓✓/⚠/✗ + Count)
  //   auf dem pattern_check-Backend. Klick auf eine Violation-Zelle öffnet die
  //   file:line-Liste; Klick auf einen Eintrag springt in den Code-Tab.
  //   Violations mit Confidence < 0.6 sind bereits serverseitig als Rauschen gefiltert.

  let results: PatternResult[] = [];
  let loading = false;
  let error: string | null = null;
  let selected: HeatmapCell | null = null;

  $: heatmap = buildHeatmap(results);

  async function load() {
    loading = true;
    error = null;
    selected = null;
    try {
      results = await patternCheck();
    } catch (err) {
      error = String(err);
      results = [];
    } finally {
      loading = false;
    }
  }

  function patternLabel(id: string): string {
    return PATTERN_LABELS[id] ?? id;
  }

  function cellTitle(cell: HeatmapCell): string {
    if (cell.state === 'na') return `${cell.module}: Muster nicht anwendbar`;
    if (cell.violations === 0) return `${cell.module}: ${cell.holds} Klassen erfüllen die Regel`;
    return `${cell.module}: ${cell.violations} Verstoss/Verstösse, ${cell.holds} ok — anklicken für Details`;
  }

  function onCellClick(cell: HeatmapCell) {
    if (cell.violations === 0) {
      selected = null;
      return;
    }
    selected = cell;
  }

  // Klick auf eine Violation: passende Klasse im Store finden und im Code-Tab öffnen.
  function openViolation(fqn: string) {
    const match = get(classes).find((c) => c.fqn === fqn);
    if (match) {
      selectedClass.set(match);
      viewMode.set('classes');
    }
  }

  onMount(load);
</script>

<div class="patterns">
  <header class="patterns-bar">
    <div class="patterns-title">
      <strong>Patterns</strong>
      <span class="muted"
        >Architektur-Drift · Zeilen = Muster · Spalten = Module · ✓✓ ok / ⚠ teils / ✗ Drift</span
      >
    </div>
    <button class="reload" on:click={load} title="Neu prüfen">↺</button>
  </header>

  {#if error}
    <div class="patterns-msg error">{error}</div>
  {:else if loading && results.length === 0}
    <div class="patterns-msg">Prüfe Muster …</div>
  {:else if heatmap.modules.length === 0}
    <div class="patterns-msg">Keine Daten (kein Repo offen oder keine Spring-Klassen).</div>
  {:else}
    <div class="patterns-body">
      <div class="heatmap-wrap">
        <table class="heatmap">
          <thead>
            <tr>
              <th class="corner">Muster \ Modul</th>
              {#each heatmap.modules as module (module)}
                <th class="mod" title={module}>{module}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each heatmap.rows as row (row.pattern)}
              <tr>
                <th class="rowhead" title={`Detektor-Confidence ${row.confidence.toFixed(2)}`}>
                  {patternLabel(row.pattern)}
                </th>
                {#each row.cells as cell (cell.module)}
                  <td
                    class="cell {cell.state}"
                    class:active={selected === cell}
                    class:clickable={cell.violations > 0}
                    title={cellTitle(cell)}
                    on:click={() => onCellClick(cell)}
                    on:keydown={(e) => e.key === 'Enter' && onCellClick(cell)}
                    role={cell.violations > 0 ? 'button' : undefined}
                    tabindex={cell.violations > 0 ? 0 : undefined}
                  >
                    <span class="glyph">{cell.glyph}</span>
                    {#if cell.violations > 0}
                      <span class="count">{cell.violations}</span>
                    {:else if cell.holds > 0}
                      <span class="count muted">{cell.holds}</span>
                    {/if}
                  </td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>

      {#if selected}
        <aside class="drill">
          <header class="drill-head">
            <strong>{patternLabel(selected.pattern)}</strong>
            <span class="muted">· {selected.module} · {selected.violations} Verstoss/Verstösse</span>
            <button class="drill-close" on:click={() => (selected = null)} title="Schliessen"
              >×</button
            >
          </header>
          <ul class="viol-list">
            {#each selected.violationList as v (v.fqn + ':' + v.line)}
              <li>
                <button class="viol" on:click={() => openViolation(v.fqn)} title={v.fqn}>
                  <span class="viol-loc">{v.file}:{v.line}</span>
                  <span class="viol-msg">{v.message}</span>
                  <span class="viol-conf">conf {v.confidence.toFixed(2)}</span>
                </button>
              </li>
            {/each}
          </ul>
        </aside>
      {/if}
    </div>
  {/if}
</div>

<style>
  .patterns {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .patterns-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 10px;
    flex-wrap: wrap;
    border-bottom: 1px solid var(--border, #2a2a2a);
  }
  .patterns-title strong {
    margin-right: 8px;
  }
  .muted {
    color: var(--muted, #888);
    font-size: 12px;
  }
  .reload {
    cursor: pointer;
  }
  .patterns-msg {
    padding: 16px;
    color: var(--muted, #888);
  }
  .patterns-msg.error {
    color: #e74c3c;
    white-space: pre-wrap;
  }
  .patterns-body {
    flex: 1;
    min-height: 0;
    display: flex;
    overflow: hidden;
  }
  .heatmap-wrap {
    flex: 1;
    min-width: 0;
    overflow: auto;
    padding: 10px;
  }
  .heatmap {
    border-collapse: collapse;
    font-size: 13px;
  }
  .heatmap th,
  .heatmap td {
    border: 1px solid var(--border, #2a2a2a);
    padding: 4px 8px;
  }
  .heatmap thead th {
    position: sticky;
    top: 0;
    background: var(--bg, #1c1c1c);
    z-index: 1;
  }
  .corner,
  .rowhead {
    text-align: left;
    white-space: nowrap;
    background: var(--bg, #1c1c1c);
  }
  .rowhead {
    position: sticky;
    left: 0;
    z-index: 1;
  }
  .mod {
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cell {
    text-align: center;
    white-space: nowrap;
    color: #fff;
  }
  .cell .glyph {
    font-weight: 700;
  }
  .cell .count {
    margin-left: 4px;
    font-variant-numeric: tabular-nums;
    font-size: 11px;
  }
  .cell.pass {
    background: hsl(120, 45%, 30%);
  }
  .cell.warn {
    background: hsl(45, 70%, 38%);
  }
  .cell.fail {
    background: hsl(0, 60%, 40%);
  }
  .cell.na {
    background: transparent;
    color: var(--muted, #888);
  }
  .cell.clickable {
    cursor: pointer;
  }
  .cell.clickable:hover,
  .cell.active {
    outline: 2px solid #fff;
    outline-offset: -2px;
  }
  .drill {
    width: 360px;
    max-width: 45%;
    border-left: 1px solid var(--border, #2a2a2a);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .drill-head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border, #2a2a2a);
  }
  .drill-close {
    margin-left: auto;
    cursor: pointer;
  }
  .viol-list {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow: auto;
    min-height: 0;
  }
  .viol {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border, #2a2a2a);
    padding: 8px 10px;
    cursor: pointer;
    color: inherit;
    font: inherit;
  }
  .viol:hover {
    background: rgba(255, 255, 255, 0.06);
  }
  .viol-loc {
    font-family: var(--mono, monospace);
    font-size: 12px;
    color: #6cb6ff;
  }
  .viol-msg {
    font-size: 12px;
  }
  .viol-conf {
    font-size: 11px;
    color: var(--muted, #888);
  }
</style>
