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
  /// Stage 3/3 of the #66 flythrough: orbit camera + click-drill (V4.6a),
  /// a first-person walk mode (V4.6b) — PointerLockControls own the look,
  /// the pure `cityWalk.ts` owns movement/collision/terrain, and a
  /// crosshair raycast reuses the exact same drill codepath as orbit —
  /// plus tour waypoints (V4.6c): while a walkthrough runs, the active
  /// step is mapped onto its building (pure `cityTour.ts`), the building
  /// is highlighted, and the orbit camera flies to it. Deliberately no
  /// view hijacking — the flight only happens while the city is the open
  /// view; a step arriving while another view is up does nothing here.
  import { onDestroy, onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { codeCityData, currentWalkthrough } from '../lib/api';
  import type { ClassEntry, Walkthrough } from '../lib/api';
  import {
    cameraFitFor,
    codeCityLayout,
    type CityBuilding,
    type CityModel,
  } from '../lib/diagrams/codeCityLayout';
  import { groundHeightAt, stepMovement, WALK_DEFAULTS } from '../lib/diagrams/cityWalk';
  import {
    cameraFlightTo,
    resolveTourTarget,
    shouldRefetchTourBody,
    tweenPose,
    type CameraPose,
  } from '../lib/diagrams/cityTour';
  import {
    classes,
    fileView,
    followingMcp,
    modules,
    repo,
    selectedClass,
    viewMode,
    walkthroughCursor,
    type WalkthroughCursor,
  } from '../lib/store';
  import { t } from '../lib/i18n';

  let container: HTMLDivElement;
  let loading = true;
  let error: string | null = null;
  let empty = false;
  /// WebGL missing (software rendering, disabled hardware acceleration):
  /// instead of three's raw "Error creating WebGL context" the stage shows
  /// an explanatory placeholder — every other diagram keeps working.
  let webglUnavailable = false;
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
  type PointerLockControlsT =
    import('three/examples/jsm/controls/PointerLockControls.js').PointerLockControls;

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

  // --- First-person walk state (V4.6b) --------------------------------------
  let PointerLock:
    | (new (camera: import('three').Camera, domElement: HTMLElement) => PointerLockControlsT)
    | null = null;
  let plc: PointerLockControlsT | null = null;
  /// Walk toggle — orbit is the default; flipping back restores this pose.
  let walkMode = false;
  /// Orbit pose saved on walk entry, restored verbatim on exit.
  let savedOrbit: { pos: [number, number, number]; target: [number, number, number] } | null =
    null;
  /// Building under the crosshair (walk-mode hover — same drill as orbit).
  let walkTarget: CityBuilding | null = null;
  /// Currently pressed movement keys (KeyboardEvent.code values).
  const keys = new Set<string>();
  /// Timestamp of the previous walk frame, for dt (ms, from setAnimationLoop).
  let lastTick = 0;

  /// Warm accent the glow lightens towards — the "freshly built" read.
  const GLOW_COLOR = '#ffd666';
  /// Cap on how far the glow shifts the facade towards the accent, so a
  /// red high-risk tower still reads red when freshly committed.
  const GLOW_MIX = 0.55;

  // --- Tour waypoints (V4.6c) ------------------------------------------------
  /// Cool accent the active tour step's building lerps towards — same
  /// colour-lerp technique as the recency glow, but cold so the two reads
  /// stay distinguishable. The mix is stronger than GLOW_MIX: the tour
  /// target must pop even on a red high-risk tower.
  const TOUR_COLOR = '#59c2ff';
  const TOUR_MIX = 0.65;
  /// Flight duration of the camera tween towards a step's building.
  const FLIGHT_MS = 1200;

  /// Last cursor seen from the walkthrough store (null = no active tour).
  let tourCursor: WalkthroughCursor | null = null;
  /// Tour body cache — re-fetched when the tour id *or* the cursor nonce
  /// changes (`shouldRefetchTourBody`), steps looked up locally.
  let tourBody: Walkthrough | null = null;
  /// Cursor nonce the cached body was fetched under. A bumped nonce means
  /// the body may have changed behind the same id (`walkthrough_append`,
  /// step rewrite) — matching on id alone would keep flying the stale tour.
  let tourBodyNonce = -1;
  /// Guards the async body fetch against out-of-order responses.
  let tourFetchSeq = 0;
  /// Resolved building of the active step (highlight + beacon + chip),
  /// plus its instance index for the colour restore.
  let tourBuilding: CityBuilding | null = null;
  let tourIndex = -1;
  /// In-progress camera flight (orbit mode only). `start` is set on the
  /// first animation frame; any user interaction aborts the flight.
  let flight: { from: CameraPose; to: CameraPose; start: number | null } | null = null;
  /// Walk-mode beacon: a translucent light pillar over the target building
  /// (flying the pointer-locked camera around would be disorienting).
  let beacon: import('three').Mesh | null = null;

  /// Sentinel thrown by buildScene when three's renderer constructor fails
  /// despite the preflight (e.g. context-count exhaustion, driver blacklist)
  /// — mountCity maps it onto the placeholder instead of the raw error text.
  const WEBGL_UNAVAILABLE = new Error('WebGL unavailable');

  /// Preflight: can this environment create a WebGL context at all?
  /// Cheaper and friendlier than letting `new T.WebGLRenderer` throw.
  function webglSupported(): boolean {
    try {
      const canvas = document.createElement('canvas');
      return !!(canvas.getContext('webgl2') ?? canvas.getContext('webgl'));
    } catch {
      return false;
    }
  }

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

      // No WebGL → explanatory placeholder, and the (heavy) three chunk
      // is never fetched at all.
      if (!webglSupported()) {
        webglUnavailable = true;
        loading = false;
        return;
      }

      // Dynamic imports — the whole three chunk lands only now.
      const [threeModule, { OrbitControls }, { PointerLockControls }] = await Promise.all([
        import('three'),
        import('three/examples/jsm/controls/OrbitControls.js'),
        import('three/examples/jsm/controls/PointerLockControls.js'),
      ]);
      three = threeModule;
      PointerLock = PointerLockControls;

      if (empty) {
        loading = false;
        return;
      }

      buildScene(three, OrbitControls, model);
      // A tour may already be running when the city opens — apply its
      // active step now that model + scene exist (V4.6c).
      applyTour();
      loading = false;
    } catch (err) {
      if (err === WEBGL_UNAVAILABLE) webglUnavailable = true;
      else error = String(err);
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

    // The preflight can pass and this still throw (context limit reached,
    // GPU process gone) — same user story, same placeholder.
    try {
      renderer = new T.WebGLRenderer({ antialias: true });
    } catch {
      scene = null;
      throw WEBGL_UNAVAILABLE;
    }
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.setSize(container.clientWidth || 1, container.clientHeight || 1);
    container.appendChild(renderer.domElement);

    // Near plane 0.1: at walking eye height the camera gets close to facades;
    // 0.5 would clip them. Depth precision stays fine (plateau steps are 0.6).
    camera = new T.PerspectiveCamera(
      50,
      (container.clientWidth || 1) / (container.clientHeight || 1),
      0.1,
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

    // Tour beacon (V4.6c): shares the unit cube, scaled per target into a
    // translucent light pillar. Basic material — the beacon is light, not
    // architecture, so it must not react to the sun.
    const beaconMat = new T.MeshBasicMaterial({
      color: TOUR_COLOR,
      transparent: true,
      opacity: 0.3,
      depthWrite: false,
    });
    disposables.push(beaconMat);
    beacon = new T.Mesh(box, beaconMat);
    beacon.visible = false;
    scene.add(beacon);

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
    renderer.domElement.addEventListener('wheel', onWheel, { passive: true });

    // Continuous loop — damping needs per-frame control updates anyway;
    // torn down via setAnimationLoop(null) in onDestroy. During a tour
    // flight the tween writes the pose first; controls.update() is a
    // no-op then (any user input cancels the flight before it applies).
    renderer.setAnimationLoop((time: number) => {
      if (walkMode) {
        stepWalkFrame(time);
      } else {
        stepFlight(time);
        controls?.update();
      }
      if (renderer && scene && camera) renderer.render(scene, camera);
    });
  }

  /// Fly the orbit camera back to the establishing shot.
  export function resetCamera() {
    if (!camera || !controls || !model || walkMode) return;
    flight = null; // user intent beats an in-progress tour flight
    const fit = cameraFitFor(model);
    camera.position.set(...fit.position);
    controls.target.set(...fit.target);
    controls.update();
  }

  // --- Tour waypoints (V4.6c) -------------------------------------------------

  /// Store subscription: every cursor move re-resolves the active step.
  /// The body is re-fetched whenever the tour id or the cursor nonce moved
  /// (`shouldRefetchTourBody` — the same nonce contract WalkthroughView
  /// follows); a stale response (a newer cursor arrived mid-fetch) is
  /// dropped via the sequence guard.
  const unsubscribeTour = walkthroughCursor.subscribe((cur) => {
    void onTourCursor(cur);
  });

  async function onTourCursor(cur: WalkthroughCursor | null) {
    tourCursor = cur;
    const seq = ++tourFetchSeq;
    if (cur && shouldRefetchTourBody(cur, tourBody, tourBodyNonce)) {
      try {
        const body = await currentWalkthrough();
        if (seq !== tourFetchSeq) return; // superseded by a newer cursor
        tourBody = body;
        tourBodyNonce = cur.nonce;
      } catch {
        tourBody = null; // no body → steps can't resolve; visuals clear below
      }
    }
    applyTour();
  }

  /// fqn → absolute source file, the ClassEntry join for
  /// `resolveTourTarget`: `building.fqn` only knows the hottest class per
  /// file (risk join) and knows nothing without git history — this index
  /// resolves every parsed class via `module.root + class.file` instead.
  /// Rebuilt lazily on store identity change (load() swaps the arrays
  /// wholesale, so a reference check is a correct invalidation).
  let classIndexCache: {
    classesRef: ClassEntry[];
    modulesRef: unknown;
    map: Map<string, string>;
  } | null = null;
  function tourClassIndex(): Map<string, string> {
    const cls = get(classes);
    const mods = get(modules);
    if (classIndexCache && classIndexCache.classesRef === cls && classIndexCache.modulesRef === mods) {
      return classIndexCache.map;
    }
    const rootById = new Map(mods.map((m) => [m.id, m.root.replace(/\/+$/, '')]));
    const map = new Map<string, string>();
    for (const c of cls) {
      const mroot = rootById.get(c.module);
      if (mroot) map.set(c.fqn, `${mroot}/${c.file}`);
    }
    classIndexCache = { classesRef: cls, modulesRef: mods, map };
    return map;
  }

  /// Resolve the active step onto a building and drive the visuals:
  /// highlight (both modes), camera flight (orbit) or beacon (walk).
  /// Steps without city geometry (diff/note/…) are stopovers — the camera
  /// holds its position and no building is marked.
  function applyTour() {
    if (!model) return; // city still loading; mountCity re-applies
    const step =
      tourCursor && tourBody && tourBody.id === tourCursor.id
        ? (tourBody.steps[tourCursor.step] ?? null)
        : null;
    const hit = step
      ? resolveTourTarget(model, step.target, get(repo)?.root ?? null, tourClassIndex())
      : null;
    const idx = hit ? model.buildings.findIndex((b) => b.id === hit.buildingId) : -1;
    setTourBuilding(idx);
    if (tourBuilding && !walkMode) startFlight(tourBuilding);
  }

  /// Swap the highlighted building: restore the previous instance colour,
  /// lerp the new one towards the tour accent (same technique as the glow).
  /// The highlight persists until the next step replaces or clears it.
  function setTourBuilding(idx: number) {
    if (idx === tourIndex) {
      tourBuilding = idx >= 0 ? (model?.buildings[idx] ?? null) : null;
      updateBeacon();
      return;
    }
    if (tourIndex >= 0) paintBuilding(tourIndex, false);
    tourIndex = idx;
    tourBuilding = idx >= 0 ? (model?.buildings[idx] ?? null) : null;
    if (idx >= 0) paintBuilding(idx, true);
    updateBeacon();
  }

  /// Recompute one instance colour from scratch (base risk colour + glow
  /// lerp — mirrors buildScene), optionally lerped towards the tour accent.
  function paintBuilding(i: number, highlighted: boolean) {
    if (!three || !buildingsMesh || !model) return;
    const b = model.buildings[i];
    if (!b) return;
    const color = new three.Color(b.color);
    if (b.glow > 0) color.lerp(new three.Color(GLOW_COLOR), b.glow * GLOW_MIX);
    if (highlighted) color.lerp(new three.Color(TOUR_COLOR), TOUR_MIX);
    buildingsMesh.setColorAt(i, color);
    if (buildingsMesh.instanceColor) buildingsMesh.instanceColor.needsUpdate = true;
  }

  /// Walk-mode beacon: a translucent pillar rising from the target's roof.
  /// Orbit mode hides it — the flight + highlight already carry the read.
  function updateBeacon() {
    if (!beacon) return;
    const b = tourBuilding;
    if (!b || !walkMode) {
      beacon.visible = false;
      return;
    }
    const thickness = Math.max(Math.min(b.w, b.d) * 0.6, 0.6);
    const pillar = 16;
    beacon.scale.set(thickness, pillar, thickness);
    beacon.position.set(b.x + b.w / 2, b.y + b.h + pillar / 2, b.z + b.d / 2);
    beacon.visible = true;
  }

  /// Tween the orbit camera towards the building's waypoint pose. The
  /// flight starts from wherever the camera currently is, so repeated
  /// steps re-centre smoothly; any user input (drag/wheel/walk) aborts it.
  function startFlight(b: CityBuilding) {
    if (!camera || !controls || !model) return;
    const from: CameraPose = {
      position: camera.position.toArray() as [number, number, number],
      target: controls.target.toArray() as [number, number, number],
    };
    flight = { from, to: cameraFlightTo(model, b, from), start: null };
  }

  /// One flight frame, driven by the shared animation loop (orbit branch).
  function stepFlight(now: number) {
    if (!flight || !camera || !controls) return;
    if (flight.start === null) flight.start = now;
    const t = (now - flight.start) / FLIGHT_MS;
    const pose = tweenPose(flight.from, flight.to, t);
    camera.position.set(...pose.position);
    controls.target.set(...pose.target);
    if (t >= 1) flight = null;
  }

  /// Wheel zoom is user intent — abort a running tour flight.
  function onWheel() {
    flight = null;
  }

  // --- First-person walk (V4.6b) ---------------------------------------------

  function toggleWalk() {
    if (walkMode) exitWalk();
    else enterWalk();
  }

  /// Switch orbit → walk: freeze OrbitControls, remember their pose, drop
  /// the camera to eye height (keeping the current heading, levelling the
  /// gaze) and grab the pointer. Esc (= pointer-lock exit) leaves the mode.
  function enterWalk() {
    if (walkMode || !three || !camera || !controls || !renderer || !model || !PointerLock) return;
    savedOrbit = {
      pos: camera.position.toArray() as [number, number, number],
      target: controls.target.toArray() as [number, number, number],
    };
    controls.enabled = false;

    // Keep the orbit heading but level the gaze and clamp into the city —
    // "drop down where you were looking". YXZ is the PointerLockControls
    // euler order, so yaw survives the round-trip through the quaternion.
    const e = new three.Euler(0, 0, 0, 'YXZ');
    e.setFromQuaternion(camera.quaternion);
    const x = Math.min(Math.max(camera.position.x, 0), model.world);
    const z = Math.min(Math.max(camera.position.z, 0), model.world);
    camera.position.set(x, groundHeightAt(model, x, z) + WALK_DEFAULTS.eyeHeight, z);
    camera.rotation.set(0, e.y, 0, 'YXZ');

    plc = new PointerLock(camera, renderer.domElement);
    plc.addEventListener('unlock', onWalkUnlock);
    hover = null;
    walkTarget = null;
    walkMode = true;
    flight = null; // walking takes over — no camera flights at street level
    updateBeacon(); // …the beacon marks the tour target instead
    lastTick = 0;
    document.addEventListener('keydown', onWalkKeyDown);
    document.addEventListener('keyup', onWalkKeyUp);
    renderer.domElement.addEventListener('mousedown', onWalkMouseDown);
    plc.lock();
  }

  /// Switch walk → orbit: tear the pointer-lock plumbing down and restore
  /// the exact orbit pose saved on entry.
  function exitWalk() {
    if (!walkMode) return;
    walkMode = false;
    walkTarget = null;
    keys.clear();
    document.removeEventListener('keydown', onWalkKeyDown);
    document.removeEventListener('keyup', onWalkKeyUp);
    renderer?.domElement.removeEventListener('mousedown', onWalkMouseDown);
    if (plc) {
      // Listener off first — plc.unlock() would re-dispatch into exitWalk.
      plc.removeEventListener('unlock', onWalkUnlock);
      plc.unlock();
      plc.dispose();
      plc = null;
    }
    if (camera && controls && savedOrbit) {
      camera.position.set(...savedOrbit.pos);
      controls.target.set(...savedOrbit.target);
    }
    savedOrbit = null;
    if (controls) {
      controls.enabled = true;
      controls.update();
    }
    updateBeacon(); // beacon is walk-only; orbit shows highlight + flight
  }

  /// Pointer lock released (Esc or focus loss) → back to orbit.
  function onWalkUnlock() {
    exitWalk();
  }

  const MOVE_CODES = new Set([
    'KeyW',
    'KeyA',
    'KeyS',
    'KeyD',
    'ArrowUp',
    'ArrowDown',
    'ArrowLeft',
    'ArrowRight',
    'ShiftLeft',
    'ShiftRight',
  ]);

  function onWalkKeyDown(e: KeyboardEvent) {
    if (!MOVE_CODES.has(e.code)) return;
    e.preventDefault(); // arrows would scroll the pane
    keys.add(e.code);
  }

  function onWalkKeyUp(e: KeyboardEvent) {
    keys.delete(e.code);
  }

  /// Walk-mode drill: while locked, a click drills into the building under
  /// the crosshair — the same `drillInto` codepath as the orbit click.
  /// Unlocked (the browser can deny the grab, e.g. Chrome's cooldown right
  /// after an Esc), a click on the stage re-requests the lock instead.
  function onWalkMouseDown(e: MouseEvent) {
    if (e.button !== 0 || !plc) return;
    if (!plc.isLocked) {
      plc.lock();
      return;
    }
    if (walkTarget) drillInto(walkTarget);
  }

  /// One walk frame: WASD intent + camera yaw → pure `stepMovement`
  /// (collision, terraces, bounds), result written back onto the camera;
  /// then the crosshair pick for the HUD.
  function stepWalkFrame(now: number) {
    if (!three || !camera || !model) return;
    const dt = lastTick === 0 ? 0 : Math.min((now - lastTick) / 1000, 0.1);
    lastTick = now;
    const e = new three.Euler(0, 0, 0, 'YXZ');
    e.setFromQuaternion(camera.quaternion);
    const pose = stepMovement(
      model,
      { x: camera.position.x, y: camera.position.y, z: camera.position.z, yaw: e.y },
      {
        forward:
          (keys.has('KeyW') || keys.has('ArrowUp') ? 1 : 0) -
          (keys.has('KeyS') || keys.has('ArrowDown') ? 1 : 0),
        strafe:
          (keys.has('KeyD') || keys.has('ArrowRight') ? 1 : 0) -
          (keys.has('KeyA') || keys.has('ArrowLeft') ? 1 : 0),
        sprint: keys.has('ShiftLeft') || keys.has('ShiftRight'),
      },
      dt,
    );
    camera.position.set(pose.x, pose.y, pose.z);

    // Crosshair raycast from the screen centre.
    if (!raycaster || !buildingsMesh) return;
    raycaster.setFromCamera(new three.Vector2(0, 0), camera);
    const hit = raycaster.intersectObject(buildingsMesh, false)[0];
    const b = hit?.instanceId !== undefined ? (model.buildings[hit.instanceId] ?? null) : null;
    if (b !== walkTarget) walkTarget = b;
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
    if (walkMode) return; // pointer-lock coordinates are stale — crosshair picks instead
    hover = pick(e);
    if (renderer) renderer.domElement.style.cursor = hover ? 'pointer' : 'grab';
  }

  function onPointerLeave() {
    hover = null;
  }

  // Click vs orbit-drag: only a press that barely moved counts as a drill.
  let downAt: { x: number; y: number } | null = null;
  function onPointerDown(e: PointerEvent) {
    if (walkMode) return; // walk clicks drill via onWalkMouseDown
    flight = null; // any press (drag start) takes the camera back
    if (e.button === 0) downAt = { x: e.clientX, y: e.clientY };
  }

  function onPointerUp(e: PointerEvent) {
    if (walkMode) return;
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
    // Tour plumbing first: stop reacting to cursor moves, drop the flight.
    unsubscribeTour();
    tourFetchSeq++; // poison in-flight body fetches
    flight = null;
    // A drill out of walk mode destroys this component while the pointer is
    // still locked — release it (and the key listeners) first.
    exitWalk();
    // WKWebView leaks GPU memory on every kind switch unless the renderer
    // and its GPU resources are torn down explicitly.
    if (renderer) {
      renderer.setAnimationLoop(null);
      renderer.domElement.removeEventListener('pointermove', onPointerMove);
      renderer.domElement.removeEventListener('pointerdown', onPointerDown);
      renderer.domElement.removeEventListener('pointerup', onPointerUp);
      renderer.domElement.removeEventListener('pointerleave', onPointerLeave);
      renderer.domElement.removeEventListener('wheel', onWheel);
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
    beacon = null;
    three = null;
  });
</script>

<div class="city-root">
  <div class="toolbar">
    <button type="button" on:click={resetCamera} title={$t('diagram.resetView')}>⌂</button>
    <button
      type="button"
      class="walk-toggle"
      class:active={walkMode}
      aria-pressed={walkMode}
      disabled={loading || !!error || empty || webglUnavailable}
      on:click={toggleWalk}
      title={$t('diagram.codeCity.walkHud')}
    >{$t('diagram.codeCity.walk')}</button>
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
    {#if tourBuilding}
      <span class="tour-chip" title={$t('diagram.codeCity.tourHint')}>
        ▶ {$t('diagram.codeCity.tour')} · {tourBuilding.label}
      </span>
    {/if}
    <span class="hint">{$t('diagram.drillHint')}</span>
  </div>
  {#if loading}
    <div class="placeholder">{$t('diagram.rendering')}</div>
  {:else if webglUnavailable}
    <div class="placeholder no-webgl">
      <span class="no-webgl-icon" aria-hidden="true">🏙</span>
      <strong class="no-webgl-title">{$t('diagram.codeCity.noWebgl.title')}</strong>
      <span>{$t('diagram.codeCity.noWebgl.body')}</span>
      <span class="no-webgl-hint">{$t('diagram.codeCity.noWebgl.hint')}</span>
    </div>
  {:else if error}
    <div class="error">⚠ {error}</div>
  {:else if empty}
    <div class="placeholder">{$t('diagram.codeCity.empty')}</div>
  {/if}
  <div
    class="stage"
    class:hidden={loading || !!error || empty || webglUnavailable}
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
    {#if walkMode}
      <div class="crosshair" aria-hidden="true"></div>
      {#if walkTarget}
        <div class="walk-target">
          <span class="tt-label">{walkTarget.label}</span>
          <span class="tt-path">{walkTarget.id}</span>
        </div>
      {/if}
      <div class="walk-hud">{$t('diagram.codeCity.walkHud')}</div>
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
  .toolbar button.walk-toggle {
    width: auto;
    padding: 0 8px;
    font-size: 11px;
  }
  .toolbar button.walk-toggle.active {
    border-color: var(--accent-2);
    color: var(--accent-2);
  }
  .toolbar button.walk-toggle:disabled {
    opacity: 0.5;
    cursor: default;
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
  /* --- Tour chip (V4.6c) — the accent matches TOUR_COLOR in the scene. --- */
  .tour-chip {
    font-size: 11px;
    color: #59c2ff;
    border: 1px solid color-mix(in srgb, #59c2ff 40%, transparent);
    border-radius: 10px;
    padding: 1px 8px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 220px;
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
  /* --- Walk-mode HUD (V4.6b) --- */
  .crosshair {
    position: absolute;
    left: 50%;
    top: 50%;
    width: 14px;
    height: 14px;
    transform: translate(-50%, -50%);
    pointer-events: none;
    z-index: 9;
  }
  .crosshair::before,
  .crosshair::after {
    content: '';
    position: absolute;
    background: rgba(255, 255, 255, 0.75);
  }
  .crosshair::before {
    left: 50%;
    top: 0;
    bottom: 0;
    width: 1px;
    transform: translateX(-50%);
  }
  .crosshair::after {
    top: 50%;
    left: 0;
    right: 0;
    height: 1px;
    transform: translateY(-50%);
  }
  .walk-target {
    position: absolute;
    left: 50%;
    top: calc(50% + 22px);
    transform: translateX(-50%);
    display: flex;
    gap: 8px;
    align-items: baseline;
    max-width: 70%;
    pointer-events: none;
    z-index: 9;
    background: color-mix(in srgb, var(--bg-1) 82%, black);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    padding: 3px 8px;
    font-size: 11px;
    color: var(--fg-0);
    white-space: nowrap;
    overflow: hidden;
  }
  .walk-hud {
    position: absolute;
    left: 50%;
    bottom: 12px;
    transform: translateX(-50%);
    pointer-events: none;
    z-index: 9;
    background: color-mix(in srgb, var(--bg-1) 75%, black);
    border: 1px solid var(--bg-3);
    border-radius: 12px;
    padding: 4px 12px;
    font-size: 11px;
    color: var(--fg-2);
    white-space: nowrap;
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
  /* --- WebGL empty state (F7) — same tone as the other placeholders,
     just with an icon + title so the "why" and the "what now" both land. */
  .no-webgl {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    max-width: 480px;
    margin: 0 auto;
    padding: 48px 24px;
  }
  .no-webgl-icon {
    font-size: 28px;
    opacity: 0.75;
  }
  .no-webgl-title {
    font-size: 14px;
    color: var(--fg-0);
  }
  .no-webgl-hint {
    font-size: 12px;
    color: var(--fg-2);
  }
</style>
