<script lang="ts">
  // Walkthrough 2.0 `pattern` step (Cockpit 2.4, #160).
  //
  // Renders one architecture-drift pattern's violation list with file:line
  // jumps. Reuses the pattern_check backend and the existing jump-to-class
  // affordance (selectedClass + viewMode) from the Pattern Lens view.
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { patternCheck, PATTERN_LABELS, type PatternViolation } from '../../lib/api';
  import { moduleFromScope, stepViolations, severityGlyph } from '../../lib/patternStep';
  import { classes, selectedClass, viewMode } from '../../lib/store';

  export let pattern: string;
  export let scope: string | null = null;
  // `view` is reserved for future render modes; only `violations` today.
  export let view: string | null = null;

  let violations: PatternViolation[] = [];
  let holds = 0;
  let loading = true;
  let error: string | null = null;

  $: moduleId = moduleFromScope(scope);
  $: label = PATTERN_LABELS[pattern.toLowerCase()] ?? pattern;

  async function load() {
    loading = true;
    error = null;
    try {
      const results = await patternCheck({ pattern, module: moduleId ?? undefined });
      const result = results[0];
      violations = result ? stepViolations(result.violations) : [];
      holds = result ? result.holds.reduce((n, h) => n + h.count, 0) : 0;
    } catch (err) {
      error = String(err);
      violations = [];
    } finally {
      loading = false;
    }
  }

  function openViolation(fqn: string) {
    const match = get(classes).find((c) => c.fqn === fqn);
    if (match) {
      selectedClass.set(match);
      viewMode.set('classes');
    }
  }

  onMount(load);
</script>

<div class="pattern-step">
  <header class="pattern-head">
    <span class="pattern-kicker">Pattern</span>
    <strong class="pattern-label">{label}</strong>
    <span class="pattern-scope">{moduleId ? `module:${moduleId}` : 'whole repo'}</span>
    {#if view && view !== 'violations'}
      <span class="pattern-view muted">· {view}</span>
    {/if}
  </header>

  {#if loading}
    <div class="msg">…</div>
  {:else if error}
    <div class="msg error">⚠ {error}</div>
  {:else if violations.length === 0}
    <div class="msg ok">✓ No violations — {holds} classes satisfy this rule.</div>
  {:else}
    <ul class="viol-list">
      {#each violations as v, i (`${v.fqn}:${v.line}:${i}`)}
        <li class="viol" class:crit={v.severity >= 3} class:warn={v.severity === 2}>
          <button class="viol-jump" on:click={() => openViolation(v.fqn)} title="Open in code">
            <span class="sev">{severityGlyph(v.severity)}</span>
            <span class="loc">{v.file}{v.line > 0 ? `:${v.line}` : ''}</span>
          </button>
          <div class="viol-msg">{v.message}</div>
          <div class="viol-fqn">{v.fqn}</div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .pattern-step {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    overflow: auto;
  }
  .pattern-head {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--border, #2a2a2a);
    background: var(--panel, #1b1b1b);
    position: sticky;
    top: 0;
  }
  .pattern-kicker {
    text-transform: uppercase;
    font-size: 0.7rem;
    letter-spacing: 0.08em;
    opacity: 0.7;
  }
  .pattern-scope {
    font-size: 0.8rem;
    opacity: 0.7;
    font-family: var(--mono, monospace);
  }
  .muted {
    opacity: 0.6;
  }
  .msg {
    padding: 1rem;
  }
  .msg.ok {
    color: #6bbf7b;
  }
  .msg.error {
    color: #e05252;
  }
  .viol-list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .viol {
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
    border-left: 3px solid transparent;
  }
  .viol.warn {
    border-left-color: #d8a13a;
  }
  .viol.crit {
    border-left-color: #e05252;
  }
  .viol-jump {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    background: none;
    border: none;
    color: var(--link, #7fb0ff);
    cursor: pointer;
    padding: 0;
    font-family: var(--mono, monospace);
    font-size: 0.85rem;
  }
  .viol-jump:hover .loc {
    text-decoration: underline;
  }
  .sev {
    font-style: normal;
  }
  .viol-msg {
    font-size: 0.85rem;
    margin-top: 0.15rem;
  }
  .viol-fqn {
    font-size: 0.72rem;
    opacity: 0.6;
    font-family: var(--mono, monospace);
  }
</style>
