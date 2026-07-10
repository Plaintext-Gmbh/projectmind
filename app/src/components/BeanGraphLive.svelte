<script lang="ts">
  /// Interactive Cytoscape bean graph (`bean-graph-live`, V3.1 / #61).
  ///
  /// Additive sibling of the Mermaid `bean-graph`: same relations, but a
  /// stateful graph the user can pan/zoom/drill instead of a static SVG.
  /// Cytoscape + the fcose layout are **dynamically imported** the first time
  /// this component mounts, so they cost 0 KB until the user opens this kind —
  /// the pure `beanGraphElements` mapping and the API wrapper carry no
  /// cytoscape import, keeping them tree-shakeable and unit-testable.
  ///
  /// Node colours mirror the Mermaid `classDef` stereotype palette
  /// (`crates/core/src/diagram.rs` STEREOTYPE_STYLES); edge styles key off
  /// RelationKind. Tapping a node drills into the class, mirroring the
  /// Mermaid `onNodeClick` path in DiagramView.svelte.
  import { onMount, onDestroy } from 'svelte';
  import { get } from 'svelte/store';
  // Type-only import — erased at build time, so it adds nothing to the bundle
  // and keeps cytoscape lazy. Used to cast the fcose layout options, which
  // the base cytoscape typings don't know about.
  import type { LayoutOptions } from 'cytoscape';
  import { beanGraphData, listChangesSince } from '../lib/api';
  import type { ClassEntry } from '../lib/api';
  import { beanGraphElements } from '../lib/diagrams/beanGraphElements';
  import type { BeanGraphElements } from '../lib/diagrams/beanGraphElements';
  import { classifyBeanGraphDiff } from '../lib/diagrams/beanGraphDiff';
  import { classes, selectedClass, viewMode } from '../lib/store';
  import { t } from '../lib/i18n';

  let container: HTMLDivElement;
  // Cytoscape has no bundled types available without the dep loaded eagerly;
  // this component is the one place we accept `any` for the live instance.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let cy: any = null;
  let loading = true;
  let error: string | null = null;
  let nodeCount = 0;
  let edgeCount = 0;
  let empty = false;

  // --- Animated diff overlay (#63 concept 3) ---------------------------------
  // The elements are kept after mount so the overlay can be (re)classified
  // without re-fetching the payload. `diffRef` empty = overlay off (plain
  // graph). When set, nodes/edges whose source files changed since the ref
  // pulse + thicken; everything else fades to ~50 %.
  let els: BeanGraphElements | null = null;
  let diffRef = '';
  let diffInput = '';
  let diffLoading = false;
  let diffError: string | null = null;
  let changedCount = 0;

  // Stereotype fill/stroke/text — byte-parity with the Rust STEREOTYPE_STYLES
  // so the interactive graph reads like the Mermaid one.
  const STEREO_STYLE: Record<string, { fill: string; stroke: string; text: string }> = {
    'stereo-service': { fill: '#163a1d', stroke: '#7ee787', text: '#cdf6cd' },
    'stereo-rest-controller': { fill: '#1a2c4d', stroke: '#79c0ff', text: '#cfe6ff' },
    'stereo-controller': { fill: '#1a2c4d', stroke: '#58a6ff', text: '#cfe6ff' },
    'stereo-repository': { fill: '#3a1d4d', stroke: '#d2a8ff', text: '#ecdcff' },
    'stereo-component': { fill: '#3d2010', stroke: '#ffa657', text: '#fbe7d3' },
    'stereo-configuration': { fill: '#4d1d1d', stroke: '#ff7b72', text: '#ffd5d2' },
    'stereo-lombok': { fill: '#262626', stroke: '#a0a0a0', text: '#dddddd' },
    'stereo-default': { fill: '#21262d', stroke: '#6e7781', text: '#c9d1d9' },
  };

  function stereoSelectors() {
    return Object.entries(STEREO_STYLE).map(([cls, s]) => ({
      selector: `node[stereoClass = "${cls}"]`,
      style: {
        'background-color': s.fill,
        'border-color': s.stroke,
        'border-width': 1.5,
        color: s.text,
      },
    }));
  }

  async function mountGraph() {
    loading = true;
    error = null;
    try {
      const payload = await beanGraphData();
      els = beanGraphElements(payload);
      nodeCount = els.nodes.length;
      edgeCount = els.edges.length;
      empty = nodeCount === 0;

      // Dynamic imports — the whole cytoscape chunk lands only now.
      const [{ default: cytoscape }, { default: fcose }] = await Promise.all([
        import('cytoscape'),
        import('cytoscape-fcose'),
      ]);
      // Registering twice throws; guard so re-mounts are safe.
      if (!(cytoscape as unknown as { __fcose?: boolean }).__fcose) {
        cytoscape.use(fcose);
        (cytoscape as unknown as { __fcose?: boolean }).__fcose = true;
      }

      if (empty) {
        loading = false;
        return;
      }

      cy = cytoscape({
        container,
        elements: [...els.nodes, ...els.edges],
        wheelSensitivity: 0.2,
        style: [
          {
            selector: 'node',
            style: {
              label: 'data(label)',
              'font-size': 9,
              'text-valign': 'center',
              'text-halign': 'center',
              'text-wrap': 'ellipsis',
              'text-max-width': '90px',
              width: 'label',
              height: '18px',
              padding: '6px',
              shape: 'round-rectangle',
              'background-color': '#21262d',
              'border-color': '#6e7781',
              'border-width': 1,
              color: '#c9d1d9',
            },
          },
          ...stereoSelectors(),
          {
            selector: 'edge',
            style: {
              width: 1,
              'line-color': '#6e7781',
              'target-arrow-color': '#6e7781',
              'target-arrow-shape': 'triangle',
              'arrow-scale': 0.7,
              'curve-style': 'bezier',
            },
          },
          // Cross-module edges get a heavier stroke (Mermaid parity).
          { selector: 'edge[?crossModule]', style: { width: 2, 'line-color': '#9da5b4' } },
          // RelationKind accents.
          {
            selector: 'edge[relClass = "rel-extends"]',
            style: { 'line-color': '#7ee787', 'target-arrow-color': '#7ee787', width: 2 },
          },
          {
            selector: 'edge[relClass = "rel-implements"]',
            style: { 'line-style': 'dashed', 'line-color': '#79c0ff', 'target-arrow-color': '#79c0ff' },
          },
          {
            selector: 'edge[relClass = "rel-calls"]',
            style: { 'line-style': 'dotted' },
          },
          {
            selector: 'node:selected',
            style: { 'border-color': '#f0f6fc', 'border-width': 3 },
          },
          // --- Diff overlay (#63 concept 3) ---------------------------------
          // Animate opacity + stroke so toggling the classes eases rather than
          // snaps. Applied to base node/edge so both directions transition.
          {
            selector: 'node',
            style: { 'transition-property': 'opacity border-width border-color', 'transition-duration': 300 },
          },
          {
            selector: 'edge',
            style: { 'transition-property': 'opacity width line-color', 'transition-duration': 300 },
          },
          // Unchanged elements recede so the changed ones read as the signal.
          { selector: '.faded', style: { opacity: 0.5 } },
          // Changed nodes: full opacity + a heavier accent stroke.
          {
            selector: 'node.changed',
            style: { opacity: 1, 'border-width': 4, 'border-color': '#f0b429' },
          },
          { selector: 'edge.changed', style: { opacity: 1, width: 3, 'line-color': '#f0b429', 'target-arrow-color': '#f0b429' } },
          // One-shot pulse: a brighter, thicker ring toggled on for ~700 ms.
          {
            selector: 'node.pulse',
            style: { 'border-width': 8, 'border-color': '#ffd666' },
          },
        ],
        // fcose-specific options aren't in the base cytoscape LayoutOptions
        // union (the extension ships no types), so cast through unknown.
        layout: {
          name: 'fcose',
          quality: 'default',
          animate: false,
          // Cluster nodes of the same module (parity with Mermaid subgraphs).
          nodeSeparation: 90,
          nodeRepulsion: 6000,
          idealEdgeLength: 70,
          packComponents: true,
        } as unknown as LayoutOptions,
      });

      cy.on('tap', 'node', (evt: { target: { id: () => string; data: (k: string) => string } }) => {
        const fqn = evt.target.id();
        const moduleId = evt.target.data('module');
        drillIntoClass(moduleId, fqn);
      });
      loading = false;
    } catch (err) {
      error = String(err);
      loading = false;
    }
  }

  /// Mirror of DiagramView's `handleNodeClick('class', …)`: find the parsed
  /// class and open it in the Classes tab.
  function drillIntoClass(moduleId: string, fqn: string) {
    const match = get(classes).find(
      (c: ClassEntry) => c.module === moduleId && c.fqn === fqn,
    );
    if (match) {
      selectedClass.set(match);
      viewMode.set('classes');
    }
  }

  export function fit() {
    cy?.fit(undefined, 30);
  }
  export function zoomBy(factor: number) {
    if (!cy) return;
    cy.zoom({ level: cy.zoom() * factor, renderedPosition: { x: cy.width() / 2, y: cy.height() / 2 } });
  }

  // --- Diff overlay wiring ---------------------------------------------------

  /// Load the change set for `diffInput` and paint the overlay. Empty input
  /// clears the overlay (plain graph). Errors (not a git repo, unknown ref)
  /// surface inline and leave the graph un-faded.
  async function applyDiff() {
    const ref = diffInput.trim();
    diffError = null;
    if (!ref) {
      clearDiff();
      return;
    }
    if (!cy || !els) return;
    diffLoading = true;
    try {
      const changes = await listChangesSince(ref);
      diffRef = ref;
      const diff = classifyBeanGraphDiff(els, changes);
      paintDiff(diff.changedNodeIds, diff.changedEdgeIds);
      changedCount = diff.changedNodeIds.size;
    } catch (err) {
      diffError = String(err);
      clearDiff();
    } finally {
      diffLoading = false;
    }
  }

  /// Toggle the `changed` / `faded` classes and fire a one-shot pulse on the
  /// changed nodes. Everything not changed fades; when nothing changed we leave
  /// the graph plain rather than dim the whole thing.
  function paintDiff(changedNodeIds: Set<string>, changedEdgeIds: Set<string>) {
    if (!cy) return;
    cy.batch(() => {
      cy.elements().removeClass('changed faded pulse');
      if (changedNodeIds.size === 0) return;
      cy.nodes().forEach((n: { id: () => string; addClass: (c: string) => void }) => {
        n.addClass(changedNodeIds.has(n.id()) ? 'changed' : 'faded');
      });
      cy.edges().forEach((e: { id: () => string; addClass: (c: string) => void }) => {
        e.addClass(changedEdgeIds.has(e.id()) ? 'changed' : 'faded');
      });
    });
    pulseChanged();
  }

  /// One-shot pulse: add the `pulse` class to changed nodes, then strip it after
  /// the transition settles so it plays once and never flickers.
  let pulseTimer: ReturnType<typeof setTimeout> | null = null;
  function pulseChanged() {
    if (!cy) return;
    if (pulseTimer) clearTimeout(pulseTimer);
    cy.nodes('.changed').addClass('pulse');
    pulseTimer = setTimeout(() => {
      cy?.nodes('.pulse').removeClass('pulse');
      pulseTimer = null;
    }, 700);
  }

  /// Remove the overlay: strip all diff classes, reset state.
  function clearDiff() {
    diffRef = '';
    changedCount = 0;
    if (pulseTimer) {
      clearTimeout(pulseTimer);
      pulseTimer = null;
    }
    cy?.elements().removeClass('changed faded pulse');
  }

  function onDiffKey(evt: KeyboardEvent) {
    if (evt.key === 'Enter') void applyDiff();
  }

  onMount(mountGraph);
  onDestroy(() => {
    if (pulseTimer) clearTimeout(pulseTimer);
    cy?.destroy();
    cy = null;
  });
</script>

<div class="bean-live-root">
  <div class="toolbar">
    <button type="button" on:click={() => zoomBy(1.25)} title={$t('diagram.zoomIn')}>＋</button>
    <button type="button" on:click={() => zoomBy(0.8)} title={$t('diagram.zoomOut')}>－</button>
    <button type="button" on:click={fit} title={$t('diagram.resetView')}>⌂</button>
    <span class="summary">{nodeCount} · {edgeCount}</span>
    {#if !empty}
      <span class="divider"></span>
      <label class="since-label" for="bean-diff-ref">{$t('diagram.beanGraphLive.since')}</label>
      <input
        id="bean-diff-ref"
        class="since-input"
        type="text"
        bind:value={diffInput}
        on:keydown={onDiffKey}
        placeholder="HEAD~10"
        title={$t('diagram.beanGraphLive.sinceTitle')}
        aria-label={$t('diagram.beanGraphLive.sinceTitle')}
      />
      <button
        type="button"
        class="since-apply"
        on:click={applyDiff}
        disabled={diffLoading}
        title={$t('diagram.beanGraphLive.applyDiff')}
      >{diffLoading ? '…' : $t('diagram.beanGraphLive.applyDiff')}</button>
      {#if diffRef}
        <button
          type="button"
          class="since-clear"
          on:click={() => { diffInput = ''; clearDiff(); }}
          title={$t('diagram.beanGraphLive.clearDiff')}
        >✕</button>
        <span class="diff-summary">{$t('diagram.beanGraphLive.changedCount', { count: changedCount })}</span>
      {/if}
      {#if diffError}
        <span class="diff-error" title={diffError}>⚠</span>
      {/if}
    {/if}
    <span class="hint">{$t('diagram.drillHint')}</span>
  </div>
  {#if loading}
    <div class="placeholder">{$t('diagram.rendering')}</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
  {:else if empty}
    <div class="placeholder">{$t('diagram.beanGraphLive.empty')}</div>
  {/if}
  <div
    class="stage"
    class:hidden={loading || !!error || empty}
    bind:this={container}
    role="img"
    aria-label={$t('diagram.beanGraphLive.aria')}
  ></div>
</div>

<style>
  .bean-live-root {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--bg-3);
    flex-shrink: 0;
    font-size: 12px;
  }
  .toolbar button {
    background: var(--bg-2);
    border: 1px solid var(--bg-3);
    color: var(--fg-0);
    border-radius: 4px;
    width: 26px;
    height: 24px;
    cursor: pointer;
  }
  .toolbar button:hover {
    border-color: var(--accent-2);
  }
  .summary {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-2);
  }
  .divider {
    width: 1px;
    align-self: stretch;
    background: var(--bg-3);
    margin: 2px 2px;
  }
  .since-label {
    font-size: 11px;
    color: var(--fg-2);
  }
  .since-input {
    width: 88px;
    height: 22px;
    padding: 0 6px;
    background: var(--bg-0);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    color: var(--fg-0);
    font-family: var(--mono);
    font-size: 11px;
  }
  .since-input:focus {
    outline: none;
    border-color: var(--accent-2);
  }
  /* Text buttons opt out of the fixed icon-button width. */
  .toolbar button.since-apply {
    width: auto;
    padding: 0 8px;
  }
  .since-apply:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .diff-summary {
    font-size: 11px;
    color: #f0b429;
  }
  .diff-error {
    color: var(--error);
    cursor: help;
  }
  .hint {
    margin-left: auto;
    font-size: 11px;
    color: var(--fg-2);
  }
  .stage {
    flex: 1;
    min-height: 0;
    background: var(--bg-0);
  }
  .stage.hidden {
    display: none;
  }
  .placeholder,
  .error {
    padding: 32px 16px;
    text-align: center;
    font-size: 13px;
    color: var(--fg-2);
  }
  .error {
    color: var(--error);
  }
</style>
