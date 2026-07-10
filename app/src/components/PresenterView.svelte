<script lang="ts">
  // Presenter Mode (Cockpit 2.6, #162).
  //
  // A full-screen slide deck over the active walk-through: bigger fonts,
  // sidebar gone, a `3 / 12` counter, and single-key navigation. All the
  // decisions live in the pure `lib/presenter.ts` reducer so the keyboard
  // contract is unit-tested; this component wires that reducer to the DOM,
  // the TTS narrator, and the risk / pattern overlays (which reuse the
  // existing Cockpit 2.4 data — nothing is re-invented here).
  import { onMount, onDestroy } from 'svelte';
  import { marked } from 'marked';
  import { currentWalkthrough, setWalkthroughStep, showClass, readFileText } from '../lib/api';
  import type { Walkthrough, WalkthroughStep, LineRange, ClassEntry } from '../lib/api';
  import { presenterActive, walkthroughCursor, errorMessage } from '../lib/store';
  import {
    initPresenter,
    reduce,
    keyToAction,
    stepCounter,
    isFirstStep,
    isLastStep,
    type PresenterState,
    type Overlay,
  } from '../lib/presenter';
  import { speak, stopSpeaking, speechAvailable } from '../lib/speech';
  import { t } from '../lib/i18n';
  import RiskStep from './walkthrough/RiskStep.svelte';
  import PatternStep from './walkthrough/PatternStep.svelte';
  import AtlasStep from './walkthrough/AtlasStep.svelte';
  import ClassViewer from './ClassViewer.svelte';

  export let cursorStep: number;

  let body: Walkthrough | null = null;
  let ps: PresenterState = initPresenter(0, cursorStep);

  // Per-target loaded content for the current step.
  let classEntry: ClassEntry | null = null;
  let classSource = '';
  let classMeta: { file: string; line_start: number; line_end: number } | null = null;
  let plainSource = '';
  let plainHighlight: LineRange[] = [];
  let targetLoading = false;
  let targetError: string | null = null;

  // A one-line hint surfaced when the OS narrator is missing.
  let narratorHint = '';

  $: step = body && body.steps[ps.step] ? body.steps[ps.step] : null;
  $: counter = stepCounter(ps);
  $: atFirst = isFirstStep(ps);
  $: atLast = isLastStep(ps);
  $: narrationHtml = step?.narration
    ? marked.parse(step.narration, { gfm: true, breaks: false })
    : '';

  async function loadBody() {
    try {
      body = await currentWalkthrough();
    } catch (err) {
      errorMessage.set(String(err));
      body = null;
    }
    ps = reduce(ps, { type: 'setTotal', total: body?.steps.length ?? 0 });
    ps = reduce(ps, { type: 'setStep', step: cursorStep });
    ps = reduce(ps, { type: 'enter' });
    await loadTarget();
  }

  // React to external cursor changes (e.g. the LLM moved the pointer, or the
  // normal WalkthroughView advanced it) without leaving presenter mode.
  $: syncExternalStep(cursorStep);
  function syncExternalStep(extStep: number) {
    if (!body) return;
    if (extStep !== ps.step) {
      ps = reduce(ps, { type: 'setStep', step: extStep });
      void loadTarget();
    }
  }

  async function loadTarget() {
    classEntry = null;
    classSource = '';
    classMeta = null;
    plainSource = '';
    plainHighlight = [];
    targetError = null;
    const s = body?.steps[ps.step] ?? null;
    const target = s?.target;
    if (!target) return;
    targetLoading = true;
    try {
      if (target.kind === 'class') {
        await loadClass(target.fqn, target.highlight ?? []);
      } else if (target.kind === 'file' && !isMarkdown(target.path)) {
        plainSource = await readFileText(target.path);
        plainHighlight = target.highlight ?? [];
      }
      // risk / pattern / atlas / diff / artifact / note self-render or need
      // no preload (see the template below).
    } catch (err) {
      targetError = String(err);
    } finally {
      targetLoading = false;
    }
    if (ps.narrator) void narrateCurrent();
  }

  async function loadClass(fqn: string, highlight: LineRange[]) {
    const r = await showClass(fqn);
    classEntry = {
      fqn,
      name: fqn.split('.').pop() ?? fqn,
      file: r.file,
      stereotypes: [],
      kind: '',
      module: '',
    };
    classSource = r.source;
    classMeta = { file: r.file, line_start: r.line_start, line_end: r.line_end };
    plainHighlight = highlight;
  }

  function isMarkdown(p: string): boolean {
    return /\.(md|markdown|mdx)$/i.test(p);
  }

  // ----- Navigation ---------------------------------------------------------

  /// Persist the pointer so the normal WalkthroughView + the LLM stay in sync
  /// with wherever the presenter is. Optimistic local update, mirroring
  /// WalkthroughView.
  async function persistStep(next: number) {
    if (!body) return;
    try {
      await setWalkthroughStep(body.id, next);
      walkthroughCursor.update((cur) => ({
        id: body!.id,
        step: next,
        nonce: (cur?.nonce ?? 0) + 1,
      }));
    } catch (err) {
      errorMessage.set(String(err));
    }
  }

  async function go(action: 'next' | 'prev') {
    const before = ps.step;
    ps = reduce(ps, { type: action });
    if (ps.step !== before) {
      await loadTarget();
      await persistStep(ps.step);
    }
  }

  // ----- Narrator + overlays ------------------------------------------------

  async function narrateCurrent() {
    stopSpeaking();
    const text = step?.narration ?? step?.title ?? '';
    const ok = await speak(text);
    if (!ok && !speechAvailable()) {
      narratorHint = $t('presenter.narrator.unavailable');
    }
  }

  function toggleNarrator() {
    ps = reduce(ps, { type: 'toggleNarrator' });
    narratorHint = '';
    if (ps.narrator) void narrateCurrent();
    else stopSpeaking();
  }

  function toggleOverlay(overlay: Overlay) {
    ps = reduce(ps, { type: 'toggleOverlay', overlay });
  }

  function cycleScale() {
    ps = reduce(ps, { type: 'cycleScale' });
  }

  function exit() {
    stopSpeaking();
    ps = reduce(ps, { type: 'exit' });
    presenterActive.set(false);
  }

  // The current class fqn, when the step targets a class/risk — drives which
  // fqn the risk / pattern overlays annotate.
  $: overlayFqn =
    step?.target.kind === 'class' || step?.target.kind === 'risk' ? step.target.fqn : null;

  // Which drift pattern the `p` overlay checks. A `pattern`-kind step carries
  // its own pattern id; for a plain class/risk step there's no single "right"
  // pattern, so we default to `layered` (the most common architectural check).
  $: overlayPattern = step?.target.kind === 'pattern' ? step.target.pattern : 'layered';

  // ----- Keyboard -----------------------------------------------------------

  function onKey(ev: KeyboardEvent) {
    const tag = (ev.target as HTMLElement | null)?.tagName?.toLowerCase();
    if (tag === 'input' || tag === 'textarea') return;
    const action = keyToAction(ev.key);
    if (!action) return;
    // Handled here — stop the event before it bubbles to App.svelte's global
    // navigation handler, so presenter keys never double-fire (the shell's
    // arrow bindings are modifier-gated today, but this hardens against that
    // changing).
    ev.preventDefault();
    ev.stopPropagation();
    switch (action.type) {
      case 'next':
        void go('next');
        break;
      case 'prev':
        void go('prev');
        break;
      case 'toggleNarrator':
        toggleNarrator();
        break;
      case 'toggleOverlay':
        toggleOverlay(action.overlay);
        break;
      case 'cycleScale':
        cycleScale();
        break;
      case 'setScale':
        ps = reduce(ps, action);
        break;
      case 'exit':
        exit();
        break;
      default:
        ps = reduce(ps, action);
    }
  }

  onMount(() => {
    window.addEventListener('keydown', onKey, true);
    void loadBody();
    return () => window.removeEventListener('keydown', onKey, true);
  });
  onDestroy(() => stopSpeaking());
</script>

<section class="presenter" style="--scale: {ps.scale};" aria-label={$t('presenter.aria')}>
  <header class="bar">
    <div class="left">
      <span class="tour-title" title={body?.title}>{body?.title ?? ''}</span>
    </div>
    <div class="right">
      <button
        class="ctl"
        class:on={ps.narrator}
        on:click={toggleNarrator}
        title={$t('presenter.narrator.toggle')}
        aria-pressed={ps.narrator}
      >🔊 n</button>
      <button
        class="ctl"
        class:on={ps.overlays.has('risk')}
        on:click={() => toggleOverlay('risk')}
        disabled={!overlayFqn}
        title={$t('presenter.overlay.risk')}
        aria-pressed={ps.overlays.has('risk')}
      >⚠ r</button>
      <button
        class="ctl"
        class:on={ps.overlays.has('pattern')}
        on:click={() => toggleOverlay('pattern')}
        disabled={!overlayFqn}
        title={$t('presenter.overlay.pattern')}
        aria-pressed={ps.overlays.has('pattern')}
      >▦ p</button>
      <button class="ctl" on:click={cycleScale} title={$t('presenter.scale')}>
        {Math.round(ps.scale * 100)}%
      </button>
      <span class="counter" aria-label={$t('presenter.counterAria')}>{counter}</span>
      <button class="ctl exit" on:click={exit} title={$t('presenter.exit')}>✕ Esc</button>
    </div>
  </header>

  {#if narratorHint}
    <div class="hint">{narratorHint}</div>
  {/if}

  <main class="stage">
    {#if step}
      <h1 class="title">{step.title}</h1>

      {#if ps.overlays.has('risk') && overlayFqn}
        <div class="overlay risk-overlay">
          <RiskStep fqn={overlayFqn} focus={null} show={[]} />
        </div>
      {/if}
      {#if ps.overlays.has('pattern') && overlayFqn}
        <div class="overlay pattern-overlay">
          <PatternStep pattern={overlayPattern} scope={null} view={null} />
        </div>
      {/if}

      {#if !ps.overlays.has('risk') && !ps.overlays.has('pattern')}
        <div class="target">
          {#if targetLoading}
            <div class="loading">{$t('presenter.loading')}</div>
          {:else if targetError}
            <div class="error">⚠ {targetError}</div>
          {:else if step.target.kind === 'class' && classEntry && classMeta}
            <ClassViewer klass={classEntry} source={classSource} meta={classMeta} highlightRanges={plainHighlight} />
          {:else if step.target.kind === 'risk'}
            <RiskStep fqn={step.target.fqn} focus={step.target.focus ?? null} show={step.target.show ?? []} />
          {:else if step.target.kind === 'pattern'}
            <PatternStep pattern={step.target.pattern} scope={step.target.scope ?? null} view={step.target.view ?? null} />
          {:else if step.target.kind === 'atlas'}
            <AtlasStep module={step.target.module ?? null} highlightFqns={step.target.highlight_fqns ?? []} />
          {:else if step.target.kind === 'file' && !isMarkdown(step.target.path)}
            <pre class="plain"><code>{#each plainSource.split('\n') as line, i (i)}{@const n = i + 1}<span
              class="line"
              class:hl={plainHighlight.some((r) => n >= r.from && n <= r.to)}
            >{line}
</span>{/each}</code></pre>
          {:else if step.target.kind === 'note'}
            <div class="note-target">📝</div>
          {:else}
            <div class="note-target">{step.target.kind}</div>
          {/if}
        </div>
      {/if}

      {#if narrationHtml}
        <article class="narration">{@html narrationHtml}</article>
      {/if}
    {:else}
      <div class="empty">{$t('presenter.empty')}</div>
    {/if}
  </main>

  <footer class="nav">
    <button class="navbtn" on:click={() => go('prev')} disabled={atFirst}>← {$t('walkthrough.prev')}</button>
    <button class="navbtn" on:click={() => go('next')} disabled={atLast}>{$t('walkthrough.next')} →</button>
  </footer>
</section>

<style>
  .presenter {
    position: fixed;
    inset: 0;
    z-index: 2000;
    display: flex;
    flex-direction: column;
    background: var(--bg-0);
    color: var(--fg-0);
    font-size: calc(1em * var(--scale));
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 20px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--bg-3);
    flex-shrink: 0;
  }
  .left {
    min-width: 0;
    overflow: hidden;
  }
  .tour-title {
    font-weight: 600;
    font-size: 0.95em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .right {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }
  .ctl {
    background: var(--bg-2);
    color: var(--fg-1);
    border: 1px solid var(--bg-3);
    border-radius: 5px;
    padding: 5px 10px;
    font: inherit;
    font-size: 0.8em;
    cursor: pointer;
  }
  .ctl:hover:not(:disabled) {
    background: var(--bg-3);
    color: var(--fg-0);
  }
  .ctl:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .ctl.on {
    background: var(--accent-2);
    color: var(--bg-0);
    border-color: var(--accent-2);
  }
  .ctl.exit:hover {
    background: var(--error, #e05252);
    color: #fff;
    border-color: var(--error, #e05252);
  }
  .counter {
    font-variant-numeric: tabular-nums;
    font-weight: 700;
    font-size: 1em;
    padding: 0 6px;
  }
  .hint {
    padding: 6px 20px;
    background: color-mix(in srgb, var(--warn) 16%, var(--bg-1));
    color: var(--fg-0);
    font-size: 0.85em;
    flex-shrink: 0;
  }
  .stage {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    padding: 24px 40px;
    gap: 16px;
  }
  .title {
    margin: 0;
    font-size: 2em;
    font-weight: 700;
    flex-shrink: 0;
  }
  .target,
  .overlay {
    flex: 1;
    min-height: 0;
    overflow: auto;
    border: 1px solid var(--bg-3);
    border-radius: 8px;
    background: var(--bg-1);
    display: flex;
    flex-direction: column;
  }
  .target :global(.root) {
    flex: 1;
    height: auto;
  }
  .plain {
    margin: 0;
    padding: 12px 16px;
    font-family: var(--mono);
    font-size: 0.9em;
    line-height: 1.55;
    overflow: auto;
  }
  .plain .line {
    display: block;
    white-space: pre;
  }
  .plain .hl {
    background: color-mix(in srgb, var(--warn) 30%, transparent);
    border-left: 3px solid var(--warn);
    padding-left: 6px;
    margin-left: -9px;
  }
  .note-target {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 3em;
    opacity: 0.4;
  }
  .narration {
    flex-shrink: 0;
    max-height: 30vh;
    overflow-y: auto;
    background: color-mix(in srgb, var(--accent-2) 8%, var(--bg-1));
    border: 1px solid var(--bg-3);
    border-radius: 8px;
    padding: 16px 24px;
    font-size: 1.05em;
    line-height: 1.6;
  }
  .narration :global(p) {
    margin: 0.4em 0;
  }
  .narration :global(code) {
    background: var(--bg-2);
    padding: 1px 6px;
    border-radius: 3px;
    font-family: var(--mono);
  }
  .loading,
  .error,
  .empty {
    padding: 24px;
    color: var(--fg-2);
  }
  .error {
    color: var(--error, #e05252);
  }
  .nav {
    display: flex;
    justify-content: space-between;
    padding: 12px 40px 20px;
    flex-shrink: 0;
  }
  .navbtn {
    background: var(--bg-2);
    color: var(--fg-0);
    border: 1px solid var(--bg-3);
    border-radius: 6px;
    padding: 10px 22px;
    font: inherit;
    font-size: 1em;
    cursor: pointer;
  }
  .navbtn:hover:not(:disabled) {
    background: var(--bg-3);
  }
  .navbtn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }
</style>
