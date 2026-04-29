<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { marked } from 'marked';
  import {
    currentWalkthrough,
    showClass,
    readFileText,
    showDiff,
    ackWalkthrough,
    currentWalkthroughFeedback,
    requestMoreWalkthrough,
    setWalkthroughStep,
    endWalkthrough,
  } from '../lib/api';
  import type {
    Walkthrough,
    WalkthroughStep,
    LineRange,
    ClassEntry,
    FeedbackEvent,
  } from '../lib/api';
  import { errorMessage, viewMode, walkthroughCursor } from '../lib/store';
  import ClassViewer from './ClassViewer.svelte';
  import FileView from './FileView.svelte';
  import DiffView from './DiffView.svelte';

  export let cursorId: string;
  export let cursorStep: number;
  export let nonce: number = 0;

  // ----- Component state ----------------------------------------------------
  let body: Walkthrough | null = null;
  let bodyLoading = true;
  let lastLoadedId: string | null = null;
  let lastNonce = -1;

  // Per-target loaded data, recomputed when the active step changes.
  let classEntry: ClassEntry | null = null;
  let classSource = '';
  let classMeta: { file: string; line_start: number; line_end: number } | null = null;
  let plainSource = '';
  let plainHighlight: LineRange[] = [];
  let diffText = '';
  let targetLoading = false;
  let targetError: string | null = null;

  // Zoom for walk-through-owned detail text. Embedded ClassViewer, FileView
  // and DiffView keep their own zoom handling.
  const ZOOM_KEY = 'projectmind.walkthrough.detail.zoom';
  const ZOOM_MIN = 0.6;
  const ZOOM_MAX = 2.0;
  const ZOOM_STEP = 0.1;
  let detailZoom = readZoom();
  let plainEl: HTMLPreElement | null = null;
  let narrationEl: HTMLElement | null = null;

  // UI flow state.
  let feedbackPrompt = false;
  let feedbackText = '';
  let feedbackSubmitting = false;
  /// Set after the user submits "Bitte genauer beschreiben" — the UI
  /// shows a waiting card until either a new tour arrives (auto-clear) or
  /// the user opts to keep going manually.
  let waitingForFollowup = false;
  /// The question the user typed into the feedback form, preserved so the
  /// waiting card can show it back AND include it in the copy-to-CLI hint.
  /// Cleared whenever a new tour starts.
  let lastQuestion = '';
  let activeFeedbackKey = '';
  let dismissedFeedbackKey = '';
  let feedbackPoll: ReturnType<typeof setInterval> | null = null;
  /// Set after the user acks the LAST step — the UI shows a "Tour
  /// abgeschlossen" card with an explicit "Schliessen" button.
  let tourFinished = false;

  $: void load(cursorId, cursorStep, nonce);
  $: step = body && body.steps[cursorStep] ? body.steps[cursorStep] : null;
  $: total = body?.steps.length ?? 0;
  $: progressPct = total === 0 ? 0 : Math.round(((cursorStep + 1) / total) * 100);
  $: isLastStep = body !== null && cursorStep === body.steps.length - 1;

  async function load(id: string, _step: number, n: number) {
    // Already applied this exact intent.
    if (n === lastNonce) return;
    lastNonce = n;

    // New tour id detected — reset the waiting/finished flags AND fetch
    // the new body. Inside the same tour, body is unchanged (only the
    // pointer moves), so we skip the network round-trip.
    const isNewTour = id !== lastLoadedId || body === null;
    if (isNewTour) {
      waitingForFollowup = false;
      tourFinished = false;
      feedbackPrompt = false;
      feedbackText = '';
      lastQuestion = '';
      activeFeedbackKey = '';
      dismissedFeedbackKey = '';
      bodyLoading = true;
      try {
        body = await currentWalkthrough();
        lastLoadedId = body?.id ?? null;
      } catch (err) {
        errorMessage.set(String(err));
        body = null;
      } finally {
        bodyLoading = false;
      }
    }
    await loadTargetForCurrentStep();
  }

  async function loadTargetForCurrentStep() {
    classEntry = null;
    classSource = '';
    classMeta = null;
    plainSource = '';
    plainHighlight = [];
    diffText = '';
    targetError = null;

    const t = step?.target;
    if (!t) return;
    targetLoading = true;
    try {
      switch (t.kind) {
        case 'class':
          await loadClass(t.fqn);
          break;
        case 'file':
          if (!isMarkdown(t.path)) {
            plainSource = await readFileText(t.path);
            plainHighlight = t.highlight ?? [];
          }
          // Markdown: FileView handles its own loading.
          break;
        case 'diff':
          diffText = await showDiff(t.reference, t.to ?? undefined);
          break;
        case 'note':
          break;
      }
    } catch (err) {
      targetError = String(err);
    } finally {
      targetLoading = false;
    }
  }

  async function loadClass(fqn: string) {
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
  }

  function isMarkdown(p: string): boolean {
    return /\.(md|markdown|mdx)$/i.test(p);
  }

  function basename(p: string): string {
    const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
    return idx === -1 ? p : p.slice(idx + 1);
  }

  // ----- Navigation ---------------------------------------------------------

  async function goTo(idx: number) {
    if (!body) return;
    const clamped = Math.max(0, Math.min(idx, body.steps.length - 1));
    if (clamped === cursorStep) return;
    const tourId = body.id;
    try {
      await setWalkthroughStep(tourId, clamped);
      // Optimistic local update — don't wait on the file watcher (fsevents
      // can lag noticeably on macOS). The watcher's later state-changed
      // event is filtered out by App.svelte's lastSeq, so we won't
      // double-apply.
      walkthroughCursor.update((cur) => ({
        id: tourId,
        step: clamped,
        nonce: (cur?.nonce ?? 0) + 1,
      }));
    } catch (err) {
      errorMessage.set(String(err));
    }
  }

  async function next() {
    await goTo(cursorStep + 1);
  }
  async function prev() {
    await goTo(cursorStep - 1);
  }

  // ----- Feedback -----------------------------------------------------------

  async function understood() {
    if (!body) return;
    feedbackSubmitting = true;
    try {
      await ackWalkthrough(body.id, cursorStep);
      if (isLastStep) {
        tourFinished = true;
      } else {
        await goTo(cursorStep + 1);
      }
    } catch (err) {
      errorMessage.set(String(err));
    } finally {
      feedbackSubmitting = false;
    }
  }

  async function askMore() {
    feedbackPrompt = true;
    await tick();
    document.getElementById('wt-feedback-input')?.focus();
  }

  async function submitMore() {
    if (!body) return;
    feedbackSubmitting = true;
    const question = feedbackText.trim();
    try {
      await requestMoreWalkthrough(body.id, cursorStep, question || null);
      lastQuestion = question;
      feedbackPrompt = false;
      feedbackText = '';
      waitingForFollowup = true;
    } catch (err) {
      errorMessage.set(String(err));
    } finally {
      feedbackSubmitting = false;
    }
  }

  function cancelMore() {
    feedbackPrompt = false;
    feedbackText = '';
  }

  /// User decides not to wait — close the waiting card and resume on the
  /// current step. The feedback event is still in the log, so the LLM
  /// can react later.
  function dismissWaiting() {
    dismissedFeedbackKey = activeFeedbackKey;
    waitingForFollowup = false;
  }

  /// Close the tour. Removes body + feedback, returns the GUI to the
  /// empty welcome screen.
  async function closeTour() {
    try {
      await endWalkthrough();
      walkthroughCursor.set(null);
      viewMode.set('classes');
    } catch (err) {
      errorMessage.set(String(err));
    }
  }

  // CLI hand-off — phrase the user pastes into their LLM CLI. If they
  // typed a specific question, ship it inline so the LLM has full context
  // without having to poll walkthrough_feedback first.
  $: cliHint = lastQuestion
    ? `projectmind nochmal: „${lastQuestion}" — bitte als fokussierte Folge-Tour beantworten (walkthrough_start mit nur den relevanten Steps)`
    : 'projectmind walkthrough_feedback prüfen und Folge-Tour starten';
  let cliCopied = false;
  async function copyCli() {
    try {
      await navigator.clipboard.writeText(cliHint);
      cliCopied = true;
      setTimeout(() => (cliCopied = false), 1500);
    } catch {
      // Clipboard unavailable in some Tauri configs — just show "copied" anyway.
      cliCopied = true;
      setTimeout(() => (cliCopied = false), 1500);
    }
  }

  // Render markdown narration to HTML once per step.
  $: narrationHtml = step?.narration
    ? marked.parse(step.narration, { gfm: true, breaks: false })
    : '';

  // Keyboard shortcuts: ←/→ to navigate.
  function onKey(ev: KeyboardEvent) {
    if (feedbackPrompt || waitingForFollowup || tourFinished) return;
    const tag = (ev.target as HTMLElement | null)?.tagName?.toLowerCase();
    if (tag === 'input' || tag === 'textarea') return;
    if (ev.key === 'ArrowRight') {
      ev.preventDefault();
      void next();
    } else if (ev.key === 'ArrowLeft') {
      ev.preventDefault();
      void prev();
    }
  }

  function readZoom(): number {
    try {
      const v = parseFloat(localStorage.getItem(ZOOM_KEY) ?? '');
      if (Number.isFinite(v) && v > 0) return clampZoom(v);
    } catch {
      // ignore
    }
    return 1.0;
  }

  function clampZoom(z: number): number {
    return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(z * 100) / 100));
  }

  function setDetailZoom(z: number) {
    detailZoom = clampZoom(z);
    try {
      localStorage.setItem(ZOOM_KEY, String(detailZoom));
    } catch {
      // ignore
    }
  }

  function onWheel(ev: WheelEvent) {
    if (!ev.shiftKey) return;
    if (!(ev.target instanceof Node)) return;
    const inPlain = plainEl?.contains(ev.target) ?? false;
    const inNarration = narrationEl?.contains(ev.target) ?? false;
    if (!inPlain && !inNarration) return;
    const delta = Math.abs(ev.deltaY) >= Math.abs(ev.deltaX) ? ev.deltaY : ev.deltaX;
    if (delta === 0) return;
    ev.preventDefault();
    if (delta < 0) setDetailZoom(detailZoom + ZOOM_STEP);
    else setDetailZoom(detailZoom - ZOOM_STEP);
  }

  function feedbackKey(e: FeedbackEvent): string {
    return `${e.walkthrough_id}:${e.step}:${e.kind}:${e.ts}`;
  }

  async function syncFeedbackState() {
    if (!body || feedbackPrompt) return;
    try {
      const log = await currentWalkthroughFeedback();
      const latest = [...log.events]
        .reverse()
        .find((e) => e.walkthrough_id === body?.id && e.step === cursorStep);
      if (!latest) return;
      const key = feedbackKey(latest);
      if (latest.kind === 'more_detail') {
        if (key === dismissedFeedbackKey || waitingForFollowup) return;
        activeFeedbackKey = key;
        lastQuestion = latest.comment ?? '';
        feedbackText = '';
        waitingForFollowup = true;
      } else if (latest.kind === 'understood' && isLastStep) {
        tourFinished = true;
      }
    } catch (err) {
      errorMessage.set(String(err));
    }
  }

  onMount(() => {
    window.addEventListener('keydown', onKey);
    window.addEventListener('wheel', onWheel, { passive: false });
    feedbackPoll = setInterval(() => {
      void syncFeedbackState();
    }, 1500);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('wheel', onWheel);
      if (feedbackPoll) clearInterval(feedbackPoll);
    };
  });
</script>

{#if bodyLoading && body === null}
  <section class="state-card">
    <div class="card">
      <div class="spinner"></div>
      <h2>Lade Tour…</h2>
      <p>Hole die Schritte vom MCP-Server.</p>
    </div>
  </section>
{:else if !body}
  <section class="state-card">
    <div class="card">
      <h2>Keine aktive Tour</h2>
      <p>Bitte deine KI, eine Tour zu starten via <code>walkthrough_start</code>.</p>
    </div>
  </section>
{:else if waitingForFollowup}
  <section class="state-card">
    <div class="card waiting">
      <div class="spinner"></div>
      <h2>Die KI bereitet eine Antwort vor</h2>
      {#if lastQuestion}
        <blockquote class="user-question">
          <span class="quote-label">Deine Frage</span>
          <span class="quote-text">{lastQuestion}</span>
        </blockquote>
      {/if}
      <p>Tippe in deiner KI-CLI:</p>
      <div class="cli-hint">
        <code>{cliHint}</code>
        <button class="btn btn-ghost copy-btn" on:click={copyCli}>
          {cliCopied ? '✓ kopiert' : 'kopieren'}
        </button>
      </div>
      <p class="meta">
        Sobald die KI <code>walkthrough_start</code> erneut ruft, springt diese Ansicht
        automatisch in die Folge-Tour.
      </p>
      <div class="state-actions">
        <button class="btn btn-secondary" on:click={dismissWaiting}>
          Doch alleine weitermachen
        </button>
        <button class="btn btn-ghost" on:click={closeTour}>Tour beenden</button>
      </div>
    </div>
  </section>
{:else if tourFinished}
  <section class="state-card">
    <div class="card finished">
      <div class="check">✓</div>
      <h2>Tour abgeschlossen</h2>
      <p><strong>{body.title}</strong> — {body.steps.length} Schritte durchgegangen.</p>
      <div class="state-actions">
        <button class="btn btn-primary big" on:click={closeTour}>Schliessen</button>
        <button
          class="btn btn-secondary"
          on:click={() => {
            tourFinished = false;
            void goTo(0);
          }}
        >
          Erneut durchgehen
        </button>
      </div>
    </div>
  </section>
{:else}
  <section class="root">
    <aside class="sidebar">
      <header class="side-head">
        <div class="tour-title" title={body.title}>{body.title}</div>
        <div class="progress" title="Step {cursorStep + 1} of {total}">
          <span class="progress-fill" style="width: {progressPct}%"></span>
        </div>
        <div class="progress-text">{cursorStep + 1} / {total}</div>
      </header>
      <ol class="steps">
        {#each body.steps as s, i (i)}
          <li>
            <button
              type="button"
              class="step-btn"
              class:current={i === cursorStep}
              class:done={i < cursorStep}
              on:click={() => goTo(i)}
            >
              <span class="step-marker">
                {#if i < cursorStep}
                  ✓
                {:else if i === cursorStep}
                  ▸
                {:else}
                  {i + 1}
                {/if}
              </span>
              <span class="step-title">{s.title}</span>
            </button>
          </li>
        {/each}
      </ol>
      <footer class="side-foot">
        <button class="nav-btn" on:click={prev} disabled={cursorStep === 0}>← Prev</button>
        <button class="nav-btn" on:click={next} disabled={cursorStep >= total - 1}>Next →</button>
      </footer>
    </aside>

    <main class="main">
      {#if step}
        <header class="step-head">
          <div class="step-pos">Step {cursorStep + 1} of {total}{isLastStep ? ' (letzter)' : ''}</div>
          <h1 class="step-h">{step.title}</h1>
          {#if step.target.kind !== 'note'}
            <div class="step-target-hint">
              {#if step.target.kind === 'class'}
                Class: <code>{step.target.fqn}</code>
              {:else if step.target.kind === 'file'}
                File: <code>{basename(step.target.path)}</code>
              {:else if step.target.kind === 'diff'}
                Diff: <code>{step.target.reference}{step.target.to ? `..${step.target.to}` : ' → working tree'}</code>
              {/if}
            </div>
          {/if}
        </header>

        <div class="target">
          {#if targetLoading}
            <div class="loading">Lade Inhalt…</div>
          {:else if targetError}
            <div class="error">⚠ {targetError}</div>
          {:else if step.target.kind === 'class' && classEntry && classMeta}
            <ClassViewer
              klass={classEntry}
              source={classSource}
              meta={classMeta}
              highlightRanges={step.target.highlight ?? []}
            />
          {:else if step.target.kind === 'file' && isMarkdown(step.target.path)}
            <FileView path={step.target.path} anchor={step.target.anchor ?? null} {nonce} />
          {:else if step.target.kind === 'file'}
            <pre class="plain" bind:this={plainEl} style="font-size: {detailZoom}em;"><code>{#each plainSource.split('\n') as line, i (i)}{@const lineNo = i + 1}<span
              class="line"
              class:wt-highlight={plainHighlight.some((r) => lineNo >= r.from && lineNo <= r.to)}
            ><span class="lineno">{lineNo}</span><span class="content">{line}</span>
</span>{/each}</code></pre>
          {:else if step.target.kind === 'diff'}
            <DiffView reference={step.target.reference} to={step.target.to ?? null} />
          {/if}
        </div>

        {#if narrationHtml}
          <article class="narration" bind:this={narrationEl}>
            <header class="narration-head">
              <span class="narration-icon">📖</span>
              <span class="narration-label">Erklärung der KI</span>
            </header>
            <div class="narration-body" style="font-size: {detailZoom}em;">{@html narrationHtml}</div>
          </article>
        {/if}

        <footer class="actions">
          {#if feedbackPrompt}
            <div class="feedback-form">
              <label for="wt-feedback-input" class="feedback-label">
                Was soll die KI genauer beschreiben? (optional)
              </label>
              <textarea
                id="wt-feedback-input"
                bind:value={feedbackText}
                placeholder={'z.B. „Die Highlight-Zeilen 12–18 sind mir noch unklar."'}
                rows="3"
              ></textarea>
              <div class="feedback-actions">
                <button class="btn btn-secondary" on:click={cancelMore} disabled={feedbackSubmitting}>
                  Abbrechen
                </button>
                <button class="btn btn-primary" on:click={submitMore} disabled={feedbackSubmitting}>
                  {feedbackSubmitting ? '…' : 'Senden'}
                </button>
              </div>
            </div>
          {:else}
            <button
              class="btn btn-primary big"
              on:click={understood}
              disabled={feedbackSubmitting || bodyLoading}
            >
              {feedbackSubmitting ? '…' : isLastStep ? '✓ Verstanden — Tour beenden' : '✓ Verstanden'}
            </button>
            <button
              class="btn btn-secondary big"
              on:click={askMore}
              disabled={feedbackSubmitting || bodyLoading}
            >
              ? Bitte genauer beschreiben
            </button>
          {/if}
        </footer>
      {/if}
    </main>
  </section>
{/if}

<style>
  /* ----- State cards (loading / waiting / finished) ---------------------- */
  .state-card {
    display: flex;
    flex: 1;
    align-items: center;
    justify-content: center;
    padding: 32px;
    background: var(--bg-0);
  }
  .card {
    max-width: 560px;
    text-align: center;
    background: var(--bg-1);
    border: 1px solid var(--bg-3);
    border-radius: 10px;
    padding: 36px 40px;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.18);
  }
  .card h2 {
    margin: 0 0 8px;
    color: var(--fg-0);
    font-size: 20px;
    font-weight: 600;
  }
  .card p {
    color: var(--fg-1);
    margin: 8px 0;
    line-height: 1.55;
  }
  .card.waiting {
    border-color: var(--accent-2);
    background: color-mix(in srgb, var(--accent-2) 6%, var(--bg-1));
  }
  .card.finished {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, var(--bg-1));
  }
  .card .check {
    font-size: 48px;
    color: var(--accent);
    line-height: 1;
    margin-bottom: 8px;
  }
  .card .meta {
    margin-top: 14px;
    font-size: 12px;
    color: var(--fg-2);
  }

  .spinner {
    width: 28px;
    height: 28px;
    border: 3px solid var(--bg-3);
    border-top-color: var(--accent-2);
    border-radius: 50%;
    margin: 0 auto 16px;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .user-question {
    margin: 14px 0;
    padding: 12px 16px;
    background: var(--bg-0);
    border-left: 3px solid var(--accent-2);
    border-radius: 4px;
    text-align: left;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .quote-label {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--accent-2);
    font-weight: 600;
  }
  .quote-text {
    font-size: 14px;
    color: var(--fg-0);
    line-height: 1.45;
  }

  .cli-hint {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 6px 0 4px;
    padding: 10px 14px;
    background: var(--bg-0);
    border: 1px solid var(--bg-3);
    border-radius: 6px;
    text-align: left;
  }
  .cli-hint code {
    flex: 1;
    font-family: var(--mono);
    font-size: 12px;
    color: var(--fg-0);
    line-height: 1.5;
    word-break: break-word;
    white-space: pre-wrap;
  }

  .state-actions {
    margin-top: 22px;
    display: flex;
    gap: 10px;
    justify-content: center;
    flex-wrap: wrap;
  }

  /* ----- Main layout ----------------------------------------------------- */
  .root {
    display: grid;
    grid-template-columns: 280px 1fr;
    height: 100%;
    overflow: hidden;
  }

  .sidebar {
    display: flex;
    flex-direction: column;
    background: var(--bg-1);
    border-right: 1px solid var(--bg-3);
    overflow: hidden;
  }

  .side-head {
    padding: 14px 16px 12px;
    border-bottom: 1px solid var(--bg-3);
  }
  .tour-title {
    font-weight: 600;
    font-size: 14px;
    color: var(--fg-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .progress {
    margin-top: 8px;
    height: 4px;
    background: var(--bg-2);
    border-radius: 2px;
    overflow: hidden;
  }
  .progress-fill {
    display: block;
    height: 100%;
    background: var(--accent-2);
    transition: width 200ms ease;
  }
  .progress-text {
    margin-top: 4px;
    font-size: 11px;
    color: var(--fg-2);
    font-variant-numeric: tabular-nums;
  }

  .steps {
    list-style: none;
    margin: 0;
    padding: 8px 0;
    overflow-y: auto;
    flex: 1;
  }
  .step-btn {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    width: 100%;
    text-align: left;
    background: transparent;
    border: 0;
    border-left: 3px solid transparent;
    padding: 8px 14px;
    color: var(--fg-1);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }
  .step-btn:hover {
    background: var(--bg-2);
    color: var(--fg-0);
  }
  .step-btn.current {
    background: color-mix(in srgb, var(--accent-2) 18%, transparent);
    border-left-color: var(--accent-2);
    color: var(--fg-0);
    font-weight: 500;
  }
  .step-btn.done .step-marker {
    color: var(--accent);
  }
  .step-marker {
    flex-shrink: 0;
    width: 18px;
    text-align: center;
    color: var(--fg-2);
    font-variant-numeric: tabular-nums;
  }
  .step-title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .side-foot {
    display: flex;
    gap: 6px;
    padding: 10px 12px;
    border-top: 1px solid var(--bg-3);
  }
  .nav-btn {
    flex: 1;
    background: var(--bg-2);
    color: var(--fg-1);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    padding: 5px 8px;
    font-size: 12px;
    cursor: pointer;
  }
  .nav-btn:hover:not(:disabled) {
    background: var(--bg-3);
    color: var(--fg-0);
  }
  .nav-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .main {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .step-head {
    padding: 16px 28px 12px;
    border-bottom: 1px solid var(--bg-3);
    background: var(--bg-1);
    flex-shrink: 0;
  }
  .step-pos {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--fg-2);
    font-weight: 600;
  }
  .step-h {
    margin: 4px 0 6px;
    font-size: 22px;
    font-weight: 600;
    color: var(--fg-0);
  }
  .step-target-hint {
    font-size: 11px;
    color: var(--fg-2);
  }
  .step-target-hint code {
    font-family: var(--mono);
    background: var(--bg-2);
    padding: 1px 6px;
    border-radius: 3px;
  }

  .target {
    flex: 1;
    overflow: hidden;
    min-height: 200px;
    border-bottom: 1px solid var(--bg-3);
    display: flex;
    flex-direction: column;
  }
  .loading,
  .error {
    padding: 20px 28px;
    color: var(--fg-2);
  }
  .error {
    color: var(--error);
  }

  .target :global(.root) {
    flex: 1;
    height: auto;
  }
  .target .plain {
    margin: 0;
    padding: 12px 0;
    background: var(--bg-1);
    overflow: auto;
    flex: 1;
    font-family: var(--mono);
    font-size: 0.9em;
    line-height: 1.55;
  }
  .target .plain .line {
    display: block;
    padding: 0 12px;
  }
  .target .plain .lineno {
    display: inline-block;
    width: 36px;
    color: var(--fg-2);
    text-align: right;
    margin-right: 12px;
    user-select: none;
  }
  .target .plain .content {
    white-space: pre;
  }
  .target .plain .wt-highlight {
    background: color-mix(in srgb, var(--warn) 30%, transparent);
    border-left: 3px solid var(--warn);
    padding-left: 9px;
  }

  .narration {
    background: color-mix(in srgb, var(--accent-2) 8%, var(--bg-1));
    border-bottom: 1px solid var(--bg-3);
    padding: 18px 28px 14px;
    max-height: 36vh;
    overflow-y: auto;
    flex-shrink: 0;
  }
  .narration-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
  }
  .narration-icon {
    font-size: 18px;
  }
  .narration-label {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--accent-2);
    font-weight: 600;
  }
  .narration-body {
    font-size: 1em;
    line-height: 1.6;
    color: var(--fg-0);
    max-width: 820px;
  }
  .narration-body :global(p) { margin: 0.5em 0; }
  .narration-body :global(code) {
    background: var(--bg-2);
    padding: 1px 6px;
    border-radius: 3px;
    font-family: var(--mono);
    font-size: 0.9em;
  }
  .narration-body :global(pre) {
    background: var(--bg-2);
    padding: 10px 14px;
    border-radius: 4px;
    overflow-x: auto;
  }
  .narration-body :global(strong) { color: var(--fg-0); }
  .narration-body :global(em) { color: var(--fg-1); }
  .narration-body :global(ul),
  .narration-body :global(ol) { padding-left: 1.5em; }

  .actions {
    display: flex;
    gap: 12px;
    padding: 14px 28px;
    background: var(--bg-1);
    align-items: flex-start;
    flex-wrap: wrap;
    flex-shrink: 0;
  }

  /* ----- Buttons --------------------------------------------------------- */
  .btn {
    border: 1px solid transparent;
    border-radius: 6px;
    padding: 9px 18px;
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    line-height: 1.2;
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .btn.big {
    padding: 11px 22px;
    font-size: 14px;
  }
  .btn-primary {
    background: var(--accent-2);
    color: var(--bg-0);
    border-color: var(--accent-2);
  }
  .btn-primary:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-2) 80%, white);
  }
  .btn-secondary {
    background: var(--bg-2);
    color: var(--fg-0);
    border-color: var(--bg-3);
  }
  .btn-secondary:hover:not(:disabled) {
    background: var(--bg-3);
    border-color: var(--fg-2);
  }
  .btn-ghost {
    background: transparent;
    color: var(--fg-2);
    border-color: transparent;
    padding: 6px 10px;
    font-size: 12px;
  }
  .btn-ghost:hover:not(:disabled) {
    background: var(--bg-2);
    color: var(--fg-0);
  }
  .copy-btn {
    flex-shrink: 0;
  }

  .feedback-form {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .feedback-label {
    font-size: 12px;
    color: var(--fg-2);
  }
  .feedback-form textarea {
    background: var(--bg-0);
    color: var(--fg-0);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    padding: 8px 10px;
    font: inherit;
    font-size: 13px;
    resize: vertical;
  }
  .feedback-form textarea:focus {
    outline: none;
    border-color: var(--accent-2);
  }
  .feedback-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }
</style>
