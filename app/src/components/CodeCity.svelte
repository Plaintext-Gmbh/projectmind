<script lang="ts">
  /// 3D code city (`code-city`, V4.6a / #66) — the repository as a city:
  /// treemap districts = folders (terraced plateaus), buildings = files,
  /// height = sloc/bytes, facade colour = risk score, glow = fresh commits.
  ///
  /// Exactly the BeanGraphLive pattern: an imperative canvas library
  /// (Three.js) is **dynamically imported** on first mount so it costs 0 KB
  /// until the user opens this kind (vite splits it into its own `three`
  /// chunk). All geometry math lives in the pure, unit-tested
  /// `codeCityLayout.ts` — this component only maps the model onto
  /// InstancedMesh instances and wires orbit + picking.
  ///
  /// Stage 1/3 of the #66 flythrough: orbit camera + click-drill only.
  /// First-person walk (V4.6b) and tour waypoints (V4.6c) come on top.
  import { onDestroy, onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { codeCityData } from '../lib/api';
  import type { ClassEntry } from '../lib/api';
  import {
    cameraFitFor,
    codeCityLayout,
    type CityBuilding,
    type CityModel,
  } from '../lib/diagrams/codeCityLayout';
  import { classes, fileView, followingMcp, repo, selectedClass, viewMode } from '../lib/store';
  import { t } from '../lib/i18n';

  let container: HTMLDivElement;
  let loading = true;
  let error: string | null = null;
  let empty = false;
  let hasRisk = false;
  let truncated = false;
  let buildingCount = 0;
  let districtCount = 0;

  /// Hovered building + tooltip anchor (container-relative CSS px).
  let hover: { b: CityBuilding; x: number; y: number } | null = null;

  // --- Three.js state -------------------------------------------------------
  // Loaded dynamically; the type-only aliases below are erased at build time
  // so they don't drag the chunk into the eager bundle.
  type Three = typeof import('three');
  type OrbitControlsT = import('three/examples/jsm/controls/OrbitControls.js').OrbitControls;

  let three: Three | null = null;
  let model: CityModel | null = null;
  let renderer: import('three').WebGLRenderer | null = null;
  let scene: import('three').Scene | null = null;
  let camera: import('three').PerspectiveCamera | null = null;
  let controls: OrbitControlsT | null = null;
  let buildingsMesh: import('three').InstancedMesh | null = null;
  let raycaster: import('three').Raycaster | null = null;
  let disposables: { dispose(): void }[] = [];
  let resizeObserver: ResizeObserver | null = null;

  /// Warm accent the glow lightens towards — the "freshly built" read.
  const GLOW_COLOR = '#ffd666';
  /// Cap on how far the glow shifts the facade towards the accent, so a
  /// red high-risk tower still reads red when freshly committed.
  const GLOW_MIX = 0.55;

  async function mountCity() {
    loading = true;
    error = null;
    try {
      const data = await codeCityData();
      hasRisk = data.has_risk;
      model = codeCityLayout(data);
      truncated = model.truncated;
      buildingCount = model.buildings.length;
      districtCount = model.districts.length;
      empty = buildingCount === 0;

      // Dynamic imports — the whole three chunk lands only now.
      const [threeModule, { OrbitControls }] = await Promise.all([
        import('three'),
        import('three/examples/jsm/controls/OrbitControls.js'),
      ]);
      three = threeModule;

      if (empty) {
        loading = false;
        return;
      }

      buildScene(three, OrbitControls, model);
      loading = false;
    } catch (err) {
      error = String(err);
      loading = false;
    }
  }

  function buildScene(
    T: Three,
    OrbitControls: new (
      camera: import('three').Camera,
      domElement: HTMLElement,
    ) => OrbitControlsT,
    m: CityModel,
  ) {
    scene = new T.Scene();
    // Night-sky parity with the folder-map SVG background.
    scene.background = new T.Color('#090d14');

    renderer = new T.WebGLRenderer({ antialias: true });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.setSize(container.clientWidth || 1, container.clientHeight || 1);
    container.appendChild(renderer.domElement);

    camera = new T.PerspectiveCamera(
      50,
      (container.clientWidth || 1) / (container.clientHeight || 1),
      0.5,
      m.world * 10,
    );
    controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    resetCamera();

    // Lights: a soft sky/ground hemisphere plus one sun so facade colours
    // stay readable from every orbit angle.
    scene.add(new T.HemisphereLight(0xbfd8ff, 0x10141c, 1.1));
    const sun = new T.DirectionalLight(0xffffff, 1.6);
    sun.position.set(m.world * 0.6, m.world * 0.9, m.world * 0.3);
    scene.add(sun);

    // One unit cube shared by both instanced meshes; matrices scale it.
    const box = new T.BoxGeometry(1, 1, 1);
    disposables.push(box);
    const matrix = new T.Matrix4();
    const color = new T.Color();

    // Districts: flat plateau slabs, one instance per folder, brightening
    // slightly with depth so the terraces read.
    const districtMat = new T.MeshLambertMaterial({ color: 0xffffff });
    disposables.push(districtMat);
    const districts = new T.InstancedMesh(box, districtMat, m.districts.length);
    m.districts.forEach((d, i) => {
      const plinth = 0.6;
      matrix.makeScale(d.w, plinth, d.d);
      matrix.setPosition(d.x + d.w / 2, d.y - plinth / 2, d.z + d.d / 2);
      districts.setMatrixAt(i, matrix);
      districts.setColorAt(i, color.set(`hsl(215, 22%, ${9 + d.depth * 3}%)`));
    });
    districts.instanceMatrix.needsUpdate = true;
    if (districts.instanceColor) districts.instanceColor.needsUpdate = true;
    scene.add(districts);
    disposables.push(districts);

    // Buildings: one InstancedMesh for all files. Per-instance emissive is
    // not a thing without a custom shader, so the recency glow is the
    // simplest variant instead: the instance colour is lerped towards a
    // warm accent — visually "freshly built", zero shader cost.
    const buildingMat = new T.MeshLambertMaterial({ color: 0xffffff });
    disposables.push(buildingMat);
    buildingsMesh = new T.InstancedMesh(box, buildingMat, m.buildings.length);
    const glowTarget = new T.Color(GLOW_COLOR);
    m.buildings.forEach((b, i) => {
      matrix.makeScale(b.w, b.h, b.d);
      // Unit cube is centred on the origin; buildings pivot at the ground.
      matrix.setPosition(b.x + b.w / 2, b.y + b.h / 2, b.z + b.d / 2);
      buildingsMesh!.setMatrixAt(i, matrix);
      color.set(b.color);
      if (b.glow > 0) color.lerp(glowTarget, b.glow * GLOW_MIX);
      buildingsMesh!.setColorAt(i, color);
    });
    buildingsMesh.instanceMatrix.needsUpdate = true;
    if (buildingsMesh.instanceColor) buildingsMesh.instanceColor.needsUpdate = true;
    scene.add(buildingsMesh);
    disposables.push(buildingsMesh);

    raycaster = new T.Raycaster();

    resizeObserver = new ResizeObserver(() => {
      if (!renderer || !camera) return;
      const w = container.clientWidth || 1;
      const h = container.clientHeight || 1;
      renderer.setSize(w, h);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
    });
    resizeObserver.observe(container);

    renderer.domElement.addEventListener('pointermove', onPointerMove);
    renderer.domElement.addEventListener('pointerdown', onPointerDown);
    renderer.domElement.addEventListener('pointerup', onPointerUp);
    renderer.domElement.addEventListener('pointerleave', onPointerLeave);

    // Continuous loop — damping needs per-frame control updates anyway;
    // torn down via setAnimationLoop(null) in onDestroy.
    renderer.setAnimationLoop(() => {
      controls?.update();
      if (renderer && scene && camera) renderer.render(scene, camera);
    });
  }

  /// Fly the orbit camera back to the establishing shot.
  export function resetCamera() {
    if (!camera || !controls || !model) return;
    const fit = cameraFitFor(model);
    camera.position.set(...fit.position);
    controls.target.set(...fit.target);
    controls.update();
  }

  // --- Picking ---------------------------------------------------------------

  /// Raycast the pointer against the buildings mesh → hovered building.
  function pick(e: PointerEvent): { b: CityBuilding; x: number; y: number } | null {
    if (!three || !raycaster || !camera || !buildingsMesh || !model || !renderer) return null;
    const rect = renderer.domElement.getBoundingClientRect();
    const ndc = new three.Vector2(
      ((e.clientX - rect.left) / rect.width) * 2 - 1,
      -((e.clientY - rect.top) / rect.height) * 2 + 1,
    );
    raycaster.setFromCamera(ndc, camera);
    const hit = raycaster.intersectObject(buildingsMesh, false)[0];
    if (hit?.instanceId === undefined) return null;
    const b = model.buildings[hit.instanceId];
    if (!b) return null;
    return { b, x: e.clientX - rect.left, y: e.clientY - rect.top };
  }

  function onPointerMove(e: PointerEvent) {
    hover = pick(e);
    if (renderer) renderer.domElement.style.cursor = hover ? 'pointer' : 'grab';
  }

  function onPointerLeave() {
    hover = null;
  }

  // Click vs orbit-drag: only a press that barely moved counts as a drill.
  let downAt: { x: number; y: number } | null = null;
  function onPointerDown(e: PointerEvent) {
    if (e.button === 0) downAt = { x: e.clientX, y: e.clientY };
  }

  function onPointerUp(e: PointerEvent) {
    if (!downAt || e.button !== 0) return;
    const moved = Math.hypot(e.clientX - downAt.x, e.clientY - downAt.y);
    downAt = null;
    if (moved > 5) return;
    const picked = pick(e);
    if (picked) drillInto(picked.b);
  }

  /// Drill: parsed class → ClassViewer (mirror of DiagramView's
  /// `handleNodeClick('class', …)`), anything else → FileView (mirror of
  /// `openFolderNode`). `fileView.path` wants the absolute path, so the
  /// repo-relative building id is joined onto the repo root.
  function drillInto(b: CityBuilding) {
    if (b.fqn && b.module) {
      const match = get(classes).find(
        (c: ClassEntry) => c.module === b.module && c.fqn === b.fqn,
      );
      if (match) {
        selectedClass.set(match);
        viewMode.set('classes');
        return;
      }
    }
    const root = get(repo)?.root;
    if (!root) return;
    followingMcp.set(false);
    fileView.update((cur) => ({
      path: `${root}/${b.id}`,
      anchor: null,
      nonce: (cur?.nonce ?? 0) + 1,
    }));
    viewMode.set('file');
  }

  function fmtBytes(bytes: number): string {
    if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${bytes} B`;
  }

  onMount(() => {
    void mountCity();
  });

  onDestroy(() => {
    // WKWebView leaks GPU memory on every kind switch unless the renderer
    // and its GPU resources are torn down explicitly.
    if (renderer) {
      renderer.setAnimationLoop(null);
      renderer.domElement.removeEventListener('pointermove', onPointerMove);
      renderer.domElement.removeEventListener('pointerdown', onPointerDown);
      renderer.domElement.removeEventListener('pointerup', onPointerUp);
      renderer.domElement.removeEventListener('pointerleave', onPointerLeave);
      renderer.domElement.remove();
    }
    resizeObserver?.disconnect();
    controls?.dispose();
    for (const d of disposables) d.dispose();
    disposables = [];
    renderer?.dispose();
    renderer = null;
    scene = null;
    camera = null;
    controls = null;
    buildingsMesh = null;
    raycaster = null;
    three = null;
  });
</script>

<div class="city-root">
  <div class="toolbar">
    <button type="button" on:click={resetCamera} title={$t('diagram.resetView')}>⌂</button>
    <span class="summary">{buildingCount} · {districtCount}</span>
    <span class="divider"></span>
    <span class="legend">
      <span class="legend-item">▮ {$t('diagram.codeCity.legendHeight')}</span>
      {#if hasRisk}
        <span class="legend-item">
          <span class="swatch" style="background:hsl(120, 65%, 42%)"></span>
          <span class="swatch" style="background:hsl(60, 65%, 42%)"></span>
          <span class="swatch" style="background:hsl(0, 65%, 42%)"></span>
          {$t('diagram.codeCity.legendRisk')}
        </span>
      {/if}
      <span class="legend-item">
        <span class="swatch" style="background:#ffd666"></span>
        {$t('diagram.codeCity.legendGlow')}
      </span>
    </span>
    {#if truncated}
      <span class="truncated" title={$t('diagram.codeCity.truncated')}>⚠ {$t('diagram.codeCity.truncated')}</span>
    {/if}
    <span class="hint">{$t('diagram.drillHint')}</span>
  </div>
  {#if loading}
    <div class="placeholder">{$t('diagram.rendering')}</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
  {:else if empty}
    <div class="placeholder">{$t('diagram.codeCity.empty')}</div>
  {/if}
  <div
    class="stage"
    class:hidden={loading || !!error || empty}
    bind:this={container}
    role="img"
    aria-label={$t('diagram.codeCity.aria')}
  >
    {#if hover}
      <div class="tooltip" style="left: {hover.x + 14}px; top: {hover.y + 14}px">
        <div class="tt-label">{hover.b.label}</div>
        <div class="tt-path">{hover.b.id}</div>
        <div class="tt-metrics">
          {#if hover.b.sloc !== null}<span>{hover.b.sloc} sloc</span>{/if}
          <span>{fmtBytes(hover.b.bytes)}</span>
          {#if hover.b.score !== null}<span>risk {Math.round(hover.b.score)}</span>{/if}
        </div>
        {#if hover.b.fqn}
          <div class="tt-fqn">{hover.b.fqn}</div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .city-root {
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
  .legend {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 11px;
    color: var(--fg-2);
  }
  .legend-item {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .swatch {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    display: inline-block;
  }
  .truncated {
    font-size: 11px;
    color: #f0b429;
  }
  .hint {
    margin-left: auto;
    font-size: 11px;
    color: var(--fg-2);
  }
  .stage {
    flex: 1;
    min-height: 0;
    position: relative;
    background: #090d14;
    overflow: hidden;
  }
  .stage.hidden {
    display: none;
  }
  .tooltip {
    position: absolute;
    z-index: 10;
    pointer-events: none;
    max-width: 320px;
    background: color-mix(in srgb, var(--bg-1) 92%, black);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    padding: 6px 8px;
    font-size: 11px;
    color: var(--fg-0);
  }
  .tt-label {
    font-weight: 600;
    font-size: 12px;
  }
  .tt-path,
  .tt-fqn {
    font-family: var(--mono);
    color: var(--fg-2);
    overflow-wrap: anywhere;
  }
  .tt-metrics {
    display: flex;
    gap: 8px;
    margin-top: 2px;
    color: var(--fg-1, var(--fg-0));
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
