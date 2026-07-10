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
  import { resolveOverlayMode, planBeanGraphMorph } from '../lib/diagrams/beanGraphMorph';
  import { planBeanGraphFlow } from '../lib/diagrams/beanGraphFlow';
  import type { FlowPlan } from '../lib/diagrams/beanGraphFlow';
  import { classes, selectedClass, viewMode } from '../lib/store';
  import { t } from '../lib/i18n';

  /// When set, apply this ref and play the morph on mount without the user
  /// touching the toolbar. Used by the `diagram-diff` walkthrough step so a
  /// tour can "press play" on a bean-graph change (V3.3 / #125). Empty = the
  /// normal interactive component (toolbar-driven).
  export let autoMorphRef = '';
  /// Hide the since-ref toolbar controls when the component is embedded in a
  /// walkthrough step (the step supplies the ref and drives the morph itself).
  export let embedded = false;

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

  // --- Morph mode (V3.3 / #125 fourth mode) ----------------------------------
  // The morph is the animated cousin of the V3.2 diff: instead of snapping the
  // `changed`/`faded` classes on, the changed elements start recessed (small +
  // transparent, `.morph-enter`), then pulse in + thicken while the fcose
  // layout eases into place; the unchanged elements settle to the same faded
  // rest-state. After it plays the graph rests in exactly the V3.2 diff look.
  // `morphRequested` mirrors the user's mode toggle; `resolveOverlayMode`
  // turns it + `diffRef` into the active overlay state (off / diff / morph).
  let morphRequested = autoMorphRef !== '';
  $: overlayMode = resolveOverlayMode(!!diffRef, morphRequested);

  // --- Flow mode (V4.1 / #200 fourth toolbar mode) ---------------------------
  // A *simulated* request wave: BFS from the controller entry stereotypes along
  // the directed edges towards the repositories. Frontier edges become
  // marching-ants (`.flow-active`, an animated `line-dash-offset`), frontier
  // nodes reuse the existing `.pulse`, already-travelled elements dim to
  // `.flow-visited`; the plan loops until toggled off. It answers "how does a
  // request topologically travel through this system" — the tooltip is explicit
  // that this is topology order, not runtime traffic (honesty rule #61).
  //
  // Flow is NOT a since-ref overlay, so it lives in its own `flowActive` bool
  // beside the off/diff/morph `overlayMode` (resolveOverlayMode stays untouched);
  // it is mutually exclusive with the diff/morph overlay — turning one on clears
  // the other.
  let flowActive = false;

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
          // --- Morph entry start-state (V3.3) -------------------------------
          // Changed elements are parked here for one frame before the morph
          // strips the class, so the transition eases them *in* from recessed
          // (shrunk + transparent) rather than snapping to `changed`. The base
          // node/edge `transition-property` above (opacity/border/width) does
          // the easing; adding `width height` to a longer node transition would
          // fight the layout run, so we keep the recede subtle (opacity + a
          // thin border) and let the pulse carry the "arrival".
          {
            selector: 'node.morph-enter',
            style: { opacity: 0.15, 'border-width': 0.5 },
          },
          { selector: 'edge.morph-enter', style: { opacity: 0.1, width: 0.5 } },
          // --- Flow mode (V4.1 / #200) --------------------------------------
          // Frontier edges: dashed marching-ants. The dash pattern is static;
          // the rAF loop scrolls `line-dash-offset` to make the ants march. The
          // accent blue matches the controller stereotype so the wave reads as
          // "a request travelling out of the entry points".
          {
            selector: 'edge.flow-active',
            style: {
              'line-style': 'dashed',
              'line-dash-pattern': [8, 4],
              width: 3,
              'line-color': '#79c0ff',
              'target-arrow-color': '#79c0ff',
            },
          },
          // Already-travelled nodes/edges hold a slightly dimmed accent so the
          // path the wave took stays legible behind the live frontier.
          { selector: '.flow-visited', style: { opacity: 0.85 } },
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
  /// surface inline and leave the graph un-faded. When `morphRequested` is set
  /// the overlay arrives via the animated morph instead of the static paint.
  async function applyDiff() {
    const ref = diffInput.trim();
    diffError = null;
    if (!ref) {
      clearDiff();
      return;
    }
    if (!cy || !els) return;
    // Diff/morph and flow are mutually exclusive — applying a ref stops the flow.
    if (flowActive) stopFlow();
    diffLoading = true;
    try {
      const changes = await listChangesSince(ref);
      diffRef = ref;
      const diff = classifyBeanGraphDiff(els, changes);
      changedCount = diff.changedNodeIds.size;
      if (morphRequested) {
        morphIn(diff);
      } else {
        paintDiff(diff.changedNodeIds, diff.changedEdgeIds);
      }
    } catch (err) {
      diffError = String(err);
      clearDiff();
    } finally {
      diffLoading = false;
    }
  }

  /// Play the morph (V3.3): animate the changed elements *in* and settle the
  /// rest to the faded rest-state, ending in the same look `paintDiff` would
  /// produce statically. One-shot — safe to call repeatedly (each call cancels
  /// the previous timers).
  ///
  /// Mechanics, in order:
  ///  1. Build the plan (which ids enter, which recede) from the classification
  ///     plus the current graph's ids — pure, testable.
  ///  2. Park the changed elements in the recessed `.morph-enter` start-state
  ///     and set the final `changed`/`faded` classes in the same batch.
  ///  3. Next frame, strip `.morph-enter` so the base `transition-property`
  ///     eases opacity/stroke/width from recessed → full (the "enter").
  ///  4. Re-run the fcose layout with `animate: true` so the graph physically
  ///     eases into place around the change instead of jumping.
  ///  5. Fire the existing one-shot pulse on the changed nodes for the accent.
  function morphIn(diff: ReturnType<typeof classifyBeanGraphDiff>) {
    if (!cy) return;
    clearMorphTimers();

    const allNodeIds = cy.nodes().map((n: { id: () => string }) => n.id());
    const allEdgeIds = cy.edges().map((e: { id: () => string }) => e.id());
    const plan = planBeanGraphMorph(diff, allNodeIds, allEdgeIds);

    // Empty diff: leave the graph plain (parity with paintDiff's guard).
    if (!plan.animate) {
      cy.elements().removeClass('changed faded pulse morph-enter');
      return;
    }

    cy.batch(() => {
      cy.elements().removeClass('changed faded pulse morph-enter');
      cy.nodes().forEach((n: { id: () => string; addClass: (c: string) => void }) => {
        if (plan.enterNodeIds.has(n.id())) n.addClass('changed morph-enter');
        else n.addClass('faded');
      });
      cy.edges().forEach((e: { id: () => string; addClass: (c: string) => void }) => {
        if (plan.enterEdgeIds.has(e.id())) e.addClass('changed morph-enter');
        else e.addClass('faded');
      });
    });

    // Strip the start-state next frame so the transition runs (an immediate
    // remove would coincide with the add and never animate).
    morphRaf = requestAnimationFrame(() => {
      morphRaf = null;
      cy?.elements('.morph-enter').removeClass('morph-enter');
    });

    // Let the layout ease around the change. `fit: false` keeps the viewport
    // steady so the reader's eye stays on the pulsing nodes.
    cy.layout({
      name: 'fcose',
      quality: 'default',
      animate: true,
      animationDuration: 600,
      fit: false,
      randomize: false,
      nodeSeparation: 90,
      nodeRepulsion: 6000,
      idealEdgeLength: 70,
      packComponents: true,
    } as unknown as LayoutOptions).run();

    pulseChanged();
  }

  let morphRaf: ReturnType<typeof requestAnimationFrame> | null = null;
  function clearMorphTimers() {
    if (morphRaf !== null) {
      cancelAnimationFrame(morphRaf);
      morphRaf = null;
    }
  }

  /// Toggle the morph mode on/off. Flipping it while a ref is applied re-applies
  /// the overlay in the newly selected style (morph animates, plain diff snaps).
  /// Turning morph on while the flow runs stops the flow (mutually exclusive).
  function toggleMorph() {
    if (!morphRequested && flowActive) stopFlow();
    morphRequested = !morphRequested;
    if (diffRef) void applyDiff();
  }

  // --- Flow playback (V4.1 / #200) -------------------------------------------
  // The wave plan is built once per activation; a recursive setTimeout advances
  // the frontier every FLOW_WAVE_MS, and a single rAF loop scrolls the
  // marching-ants offset on the active edges. When the last wave settles we hold
  // for FLOW_LOOP_PAUSE_MS, reset every flow class, and replay from wave 0.
  const FLOW_WAVE_MS = 450;
  const FLOW_LOOP_PAUSE_MS = 1000;
  let flowPlan: FlowPlan | null = null;
  let flowWaveTimer: ReturnType<typeof setTimeout> | null = null;
  let flowRaf: ReturnType<typeof requestAnimationFrame> | null = null;
  let flowStart = 0;

  /// Toggle the flow mode. Turning it on clears any diff/morph overlay first
  /// (mutually exclusive), builds the plan, and starts the loop; turning it off
  /// stops the loop and strips every flow class back to the plain graph.
  function toggleFlow() {
    if (flowActive) {
      stopFlow();
    } else {
      startFlow();
    }
  }

  function startFlow() {
    if (!cy || !els) return;
    // Flow and the since-ref overlay are exclusive — drop diff/morph first.
    if (diffRef || morphRequested) {
      morphRequested = false;
      clearDiff();
    }
    flowActive = true;
    flowPlan = planBeanGraphFlow(els);
    if (!flowPlan.animate) {
      // Empty graph — nothing to travel; leave flowActive so the toggle reads
      // pressed, but there is no wave to play.
      return;
    }
    startFlowRaf();
    playWave(0);
  }

  /// The rAF loop: scroll `line-dash-offset` on the active edges so the dashes
  /// march. One batched `.style()` call per frame over the (small) active set.
  /// The pattern repeats every 12 px (8+4), so wrapping the offset there keeps
  /// the number bounded while staying visually continuous.
  function startFlowRaf() {
    flowStart = performance.now();
    const tick = () => {
      if (!cy || !flowActive) {
        flowRaf = null;
        return;
      }
      const elapsed = performance.now() - flowStart;
      cy.edges('.flow-active').style('line-dash-offset', -(elapsed / 16) % 12);
      flowRaf = requestAnimationFrame(tick);
    };
    flowRaf = requestAnimationFrame(tick);
  }

  /// Play wave `k`: promote the previous frontier to `.flow-visited`, light the
  /// new frontier's edges (`.flow-active`) + pulse its nodes, and fade every
  /// node/edge the wave hasn't reached yet. Recurses via setTimeout until the
  /// last wave, then pauses and loops.
  function playWave(k: number) {
    if (!cy || !flowActive || !flowPlan) return;
    const plan = flowPlan;

    if (k === 0) {
      // Fresh pass: fade everything, then reveal the entry frontier.
      cy.batch(() => {
        cy.elements().removeClass('flow-active flow-visited pulse');
        cy.elements().addClass('faded');
        const entry = cy.nodes().filter((n: { id: () => string }) => plan.waves[0].nodeIds.includes(n.id()));
        entry.removeClass('faded').addClass('flow-visited');
      });
      pulseNodes(cy.nodes().filter((n: { id: () => string }) => plan.waves[0].nodeIds.includes(n.id())));
    } else {
      const wave = plan.waves[k];
      const nodeSet = new Set(wave.nodeIds);
      const edgeSet = new Set(wave.edgeIds);
      cy.batch(() => {
        // Previous frontier's edges settle from active → visited.
        cy.edges('.flow-active').removeClass('flow-active').addClass('flow-visited');
        // This wave's carrying edges march; its nodes arrive.
        cy.edges().forEach((e: { id: () => string; removeClass: (c: string) => void; addClass: (c: string) => void }) => {
          if (edgeSet.has(e.id())) {
            e.removeClass('faded');
            e.addClass('flow-active');
          }
        });
        cy.nodes().forEach((n: { id: () => string; removeClass: (c: string) => void; addClass: (c: string) => void }) => {
          if (nodeSet.has(n.id())) {
            n.removeClass('faded');
            n.addClass('flow-visited');
          }
        });
      });
      pulseNodes(cy.nodes().filter((n: { id: () => string }) => nodeSet.has(n.id())));
    }

    const next = k + 1;
    if (next < plan.waves.length) {
      flowWaveTimer = setTimeout(() => playWave(next), FLOW_WAVE_MS);
    } else {
      // Last wave settled: hold, then replay from the top.
      flowWaveTimer = setTimeout(() => {
        if (!cy || !flowActive) return;
        cy.batch(() => cy.elements().removeClass('flow-active flow-visited pulse faded'));
        playWave(0);
      }, FLOW_LOOP_PAUSE_MS);
    }
  }

  /// Stop the flow: cancel the wave timer + rAF and strip every flow class so
  /// the plain graph is restored (mirror of `clearMorphTimers`' cleanup role).
  function stopFlow() {
    flowActive = false;
    flowPlan = null;
    if (flowWaveTimer !== null) {
      clearTimeout(flowWaveTimer);
      flowWaveTimer = null;
    }
    if (flowRaf !== null) {
      cancelAnimationFrame(flowRaf);
      flowRaf = null;
    }
    if (pulseTimer) {
      clearTimeout(pulseTimer);
      pulseTimer = null;
    }
    cy?.elements().removeClass('flow-active flow-visited pulse faded');
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

  /// One-shot pulse over a Cytoscape node collection: add the `pulse` class,
  /// then strip it after the transition settles so it plays once and never
  /// flickers. Shared by the morph (changed nodes) and the flow (each wave's
  /// frontier nodes).
  let pulseTimer: ReturnType<typeof setTimeout> | null = null;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  function pulseNodes(nodes: any) {
    if (!cy) return;
    if (pulseTimer) clearTimeout(pulseTimer);
    nodes.addClass('pulse');
    pulseTimer = setTimeout(() => {
      cy?.nodes('.pulse').removeClass('pulse');
      pulseTimer = null;
    }, 700);
  }

  /// One-shot pulse on the diff's changed nodes (morph accent).
  function pulseChanged() {
    if (!cy) return;
    pulseNodes(cy.nodes('.changed'));
  }

  /// Remove the overlay: strip all diff classes, reset state.
  function clearDiff() {
    diffRef = '';
    changedCount = 0;
    if (pulseTimer) {
      clearTimeout(pulseTimer);
      pulseTimer = null;
    }
    clearMorphTimers();
    cy?.elements().removeClass('changed faded pulse morph-enter');
  }

  function onDiffKey(evt: KeyboardEvent) {
    if (evt.key === 'Enter') void applyDiff();
  }

  onMount(async () => {
    await mountGraph();
    // When embedded in a walkthrough step with a ref, press play automatically:
    // apply the ref and morph the change in without the user touching anything.
    if (autoMorphRef && cy && els) {
      diffInput = autoMorphRef;
      morphRequested = true;
      await applyDiff();
    }
  });
  onDestroy(() => {
    if (pulseTimer) clearTimeout(pulseTimer);
    clearMorphTimers();
    stopFlow();
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
    {#if !empty && !embedded}
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
      <!-- Morph toggle (V3.3): when on, the overlay arrives via the animated
           transition instead of the static V3.2 diff paint. -->
      <button
        type="button"
        class="morph-toggle"
        class:active={overlayMode === 'morph'}
        aria-pressed={morphRequested}
        on:click={toggleMorph}
        title={$t('diagram.beanGraphLive.morphTitle')}
      >{$t('diagram.beanGraphLive.morph')}</button>
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
    <!-- Flow toggle (V4.1): a simulated request wave. Visible even when
         embedded (a tour step can play the flow), default off. Not a since-ref
         overlay, so it lives beside the off/diff/morph controls. -->
    {#if !empty}
      {#if embedded}<span class="divider"></span>{/if}
      <button
        type="button"
        class="flow-toggle"
        class:active={flowActive}
        aria-pressed={flowActive}
        on:click={toggleFlow}
        title={$t('diagram.beanGraphLive.flowTitle')}
      >{$t('diagram.beanGraphLive.flow')}</button>
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
  /* Morph toggle: a text button that lights up when the animated mode is on. */
  .toolbar button.morph-toggle {
    width: auto;
    padding: 0 8px;
  }
  .toolbar button.morph-toggle.active {
    border-color: #f0b429;
    color: #f0b429;
  }
  /* Flow toggle: text button that lights up (accent blue = the flow edge
     colour) while the simulated request wave is running. */
  .toolbar button.flow-toggle {
    width: auto;
    padding: 0 8px;
  }
  .toolbar button.flow-toggle.active {
    border-color: #79c0ff;
    color: #79c0ff;
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
