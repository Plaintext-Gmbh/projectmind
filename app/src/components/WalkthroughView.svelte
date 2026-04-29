<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { marked } from 'marked';
  import {
    currentWalkthrough,
    showClass,
    readFileText,
    showDiff,
    ackWalkthrough,
    requestMoreWalkthrough,
    setWalkthroughStep,
  } from '../lib/api';
  import type {
    Walkthrough,
    WalkthroughStep,
    WalkthroughTarget,
    LineRange,
    ClassEntry,
  } from '../lib/api';
  import { errorMessage } from '../lib/store';
  import ClassViewer from './ClassViewer.svelte';
  import FileView from './FileView.svelte';
  import DiffView from './DiffView.svelte';

  export let cursorId: string;
  export let cursorStep: number;
  export let nonce: number = 0;

  let body: Walkthrough | null = null;
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

  let feedbackPrompt = false;
  let feedbackText = '';
  let feedbackSubmitting = false;
  let lastFeedbackKind: 'understood' | 'more_detail' | null = null;

  $: void load(cursorId, cursorStep, nonce);
  $: step = body && body.steps[cursorStep] ? body.steps[cursorStep] : null;
  $: total = body?.steps.length ?? 0;
  $: progressPct = total === 0 ? 0 : Math.round(((cursorStep + 1) / total) * 100);

  async function load(id: string, _step: number, n: number) {
    if (n === lastNonce && id === lastLoadedId && body !== null) {
      // Same intent, no body refresh needed — just reload the target.
      await loadTargetForCurrentStep();
      return;
    }
    lastNonce = n;
    if (id !== lastLoadedId || body === null) {
      try {
        body = await currentWalkthrough();
        lastLoadedId = body?.id ?? null;
      } catch (err) {
        errorMessage.set(String(err));
        body = null;
      }
    }
    feedbackPrompt = false;
    feedbackText = '';
    lastFeedbackKind = null;
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
          // Markdown is rendered by FileView; for non-markdown we fetch the
          // text ourselves so we can pass `highlight` into ClassViewer's
          // wt-highlight rendering.
          if (isMarkdown(t.path)) {
            // FileView handles its own loading.
          } else {
            plainSource = await readFileText(t.path);
            plainHighlight = t.highlight ?? [];
          }
          break;
        case 'diff':
          diffText = await showDiff(t.reference, t.to ?? undefined);
          break;
        case 'note':
          // Nothing to load.
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
    try {
      await setWalkthroughStep(body.id, clamped);
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
      lastFeedbackKind = 'understood';
      // Auto-advance unless we're already at the end.
      if (cursorStep < body.steps.length - 1) {
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
    try {
      await requestMoreWalkthrough(
        body.id,
        cursorStep,
        feedbackText.trim() ? feedbackText.trim() : null,
      );
      lastFeedbackKind = 'more_detail';
      feedbackPrompt = false;
      feedbackText = '';
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

  // Render markdown narration to HTML once per step.
  $: narrationHtml = step?.narration ? marked.parse(step.narration, { gfm: true, breaks: false }) : '';

  // Keyboard shortcuts: ←/→ to navigate, Enter on focused buttons handled by browser.
  function onKey(ev: KeyboardEvent) {
    if (feedbackPrompt) return;
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

  onMount(() => {
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
</script>

{#if !body}
  <section class="empty">
    <div>
      <h2>No active walk-through</h2>
      <p>Ask the LLM to start one via <code>walkthrough_start</code>.</p>
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
          <div class="step-pos">Step {cursorStep + 1} of {total}</div>
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
            <div class="loading">Loading…</div>
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
            <FileView path={step.target.path} anchor={step.target.anchor ?? null} nonce={nonce} />
          {:else if step.target.kind === 'file' && classEntry === null}
            <pre class="plain"><code>{#each plainSource.split('\n') as line, i (i)}{@const lineNo = i + 1}<span
              class="line"
              class:wt-highlight={plainHighlight.some((r) => lineNo >= r.from && lineNo <= r.to)}
            ><span class="lineno">{lineNo}</span><span class="content">{line}</span>
</span>{/each}</code></pre>
          {:else if step.target.kind === 'diff'}
            <DiffView reference={step.target.reference} to={step.target.to ?? null} />
          {/if}
        </div>

        {#if narrationHtml}
          <article class="narration">
            <header class="narration-head">
              <span class="narration-icon">📖</span>
              <span class="narration-label">Erklärung der KI</span>
            </header>
            <div class="narration-body">{@html narrationHtml}</div>
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
          {:else if lastFeedbackKind === 'more_detail'}
            <div class="feedback-confirm">
              ✓ Rückmeldung gesendet — frag die KI in der CLI: <em>„nochmal erklären"</em>.
            </div>
          {:else}
            <button class="btn btn-primary big" on:click={understood} disabled={feedbackSubmitting}>
              ✓ Verstanden
            </button>
            <button class="btn btn-secondary big" on:click={askMore} disabled={feedbackSubmitting}>
              ? Bitte genauer beschreiben
            </button>
          {/if}
        </footer>
      {/if}
    </main>
  </section>
{/if}

<style>
  .empty {
    display: flex;
    flex: 1;
    align-items: center;
    justify-content: center;
    color: var(--fg-2);
    text-align: center;
  }
  .empty h2 {
    margin: 0 0 8px;
    color: var(--fg-0);
  }

  .root {
    display: grid;
    grid-template-columns: 280px 1fr;
    height: 100%;
    overflow: hidden;
  }

  /* Sidebar -------------------------------------------------------------- */
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

  /* Main ----------------------------------------------------------------- */
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
    font-size: 12.5px;
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

  /* Narration ------------------------------------------------------------ */
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
    font-size: 15px;
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

  /* Actions -------------------------------------------------------------- */
  .actions {
    display: flex;
    gap: 12px;
    padding: 14px 28px;
    background: var(--bg-1);
    align-items: flex-start;
    flex-wrap: wrap;
    flex-shrink: 0;
  }

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

  .feedback-confirm {
    color: var(--accent);
    font-size: 13px;
    padding: 6px 0;
  }
  .feedback-confirm em {
    color: var(--fg-0);
    font-style: italic;
  }
</style>
