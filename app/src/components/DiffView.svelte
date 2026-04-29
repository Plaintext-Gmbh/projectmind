<script lang="ts">
  import { showDiff } from '../lib/api';
<<<<<<< HEAD
  import { t } from '../lib/i18n';
=======
  import { createShiftWheelZoom } from '../lib/shiftWheelZoom';
>>>>>>> 81a01d1... refactor(ui): consolidate shift+wheel zoom into one helper

  export let reference: string;
  export let to: string | null = null;

  let raw = '';
  let lines: { kind: 'meta' | 'header' | 'add' | 'del' | 'context' | 'hunk'; text: string }[] = [];
  let loading = false;
  let error: string | null = null;

  // Shift + wheel zoom, persisted under the per-component key.
  const { zoom, action: zoomAction } = createShiftWheelZoom('projectmind.diffview.zoom');

  $: if (reference) void load(reference, to);

  async function load(ref: string, target: string | null) {
    loading = true;
    error = null;
    try {
      raw = await showDiff(ref, target ?? undefined);
      lines = parse(raw);
    } catch (err) {
      error = String(err);
      lines = [];
    } finally {
      loading = false;
    }
  }

  function parse(diff: string): typeof lines {
    return diff.split('\n').map((text) => {
      if (text.startsWith('diff --git ')) return { kind: 'header' as const, text };
      if (
        text.startsWith('--- ') ||
        text.startsWith('+++ ') ||
        text.startsWith('index ') ||
        text.startsWith('similarity index ') ||
        text.startsWith('rename from ') ||
        text.startsWith('rename to ') ||
        text.startsWith('new file ') ||
        text.startsWith('deleted file ')
      )
        return { kind: 'meta' as const, text };
      if (text.startsWith('@@')) return { kind: 'hunk' as const, text };
      if (text.startsWith('+')) return { kind: 'add' as const, text };
      if (text.startsWith('-')) return { kind: 'del' as const, text };
      return { kind: 'context' as const, text };
    });
  }
</script>

<section class="root" use:zoomAction style="font-size: {$zoom}em;">
  <header class="bar">
    <span class="kind">{$t('diff.kind')}</span>
    <code class="ref">{reference}</code>
    <span class="arrow">→</span>
    <code class="ref">{to ?? $t('diff.workingTree')}</code>
  </header>
  {#if loading}
    <div class="status">{$t('diff.computing')}</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
  {:else if lines.length === 0}
    <div class="status">{$t('diff.noChanges')}</div>
  {:else}
    <pre class="diff"><!--
   --><!-- prettier-ignore -->{#each lines as l, i (i)}<span class="line {l.kind}">{l.text || ' '}</span>
{/each}</pre>
  {/if}
</section>

<style>
  .root {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 16px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--bg-3);
    font-size: 0.86em;
    color: var(--fg-1);
  }
  .kind {
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 2px 6px;
    background: var(--bg-2);
    border-radius: 3px;
    color: var(--fg-2);
    font-size: 0.72em;
  }
  .arrow {
    color: var(--fg-2);
  }
  .ref {
    font-family: var(--mono);
    color: var(--fg-0);
    background: var(--bg-2);
    padding: 1px 6px;
    border-radius: 3px;
  }

  .status,
  .error {
    padding: 24px;
    color: var(--fg-2);
  }
  .error {
    color: var(--error);
  }

  .diff {
    margin: 0;
    padding: 16px;
    font-family: var(--mono);
    font-size: 0.86em;
    line-height: 1.45;
    overflow: auto;
    flex: 1;
    background: var(--bg-0);
    color: var(--fg-1);
    white-space: pre;
  }

  .line {
    display: block;
    padding: 0 12px;
    border-left: 3px solid transparent;
  }
  .line.header {
    color: var(--accent-2);
    font-weight: 600;
    margin-top: 1em;
    background: var(--bg-1);
    border-left-color: var(--accent-2);
  }
  .line.meta {
    color: var(--fg-2);
  }
  .line.hunk {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    border-left-color: var(--accent);
  }
  .line.add {
    color: #b8eaa6;
    background: color-mix(in srgb, #2ea043 18%, transparent);
    border-left-color: #2ea043;
  }
  .line.del {
    color: #f8b6b6;
    background: color-mix(in srgb, #cf222e 18%, transparent);
    border-left-color: #cf222e;
  }
</style>
