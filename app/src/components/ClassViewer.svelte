<script lang="ts">
  import type { ClassEntry } from '../lib/api';

  export let klass: ClassEntry;
  export let source: string;
  export let meta: { file: string; line_start: number; line_end: number } | null;
  /// Walk-through highlight ranges (1-based, inclusive). When set, these
  /// take precedence over the default class-bounds highlight and use a
  /// more vivid colour so the LLM-pointed lines stand out.
  export let highlightRanges: Array<{ from: number; to: number }> = [];

  $: lines = source.split('\n');
  $: defaultFrom = meta?.line_start ?? 0;
  $: defaultTo = meta?.line_end ?? 0;

  function inWalkthroughRange(line: number): boolean {
    return highlightRanges.some((r) => line >= r.from && line <= r.to);
  }
</script>

<div class="root">
  <div class="header">
    <div>
      <h2>{klass.name}</h2>
      <p class="fqn">{klass.fqn}</p>
    </div>
    <div class="meta">
      {#each klass.stereotypes as s}
        <span class="badge {s}">{s}</span>
      {/each}
      {#if meta}
        <span class="file">{meta.file}:{meta.line_start}–{meta.line_end}</span>
      {/if}
    </div>
  </div>

  <pre class="source"><code>{#each lines as line, i (i)}{@const lineNo = i + 1}<span
        class="line"
        class:highlight={highlightRanges.length === 0 &&
          lineNo >= defaultFrom &&
          lineNo <= defaultTo}
        class:wt-highlight={highlightRanges.length > 0 && inWalkthroughRange(lineNo)}
      ><span class="lineno">{lineNo}</span><span class="content">{line}</span>
</span>{/each}</code></pre>
</div>

<style>
  .root {
    padding: 16px 20px;
    height: 100%;
  }

  .header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 12px;
  }

  h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
  }

  .fqn {
    margin: 4px 0 0;
    font-family: var(--mono);
    font-size: 12px;
    color: var(--fg-2);
  }

  .meta {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .file {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-2);
  }

  .source {
    background: var(--bg-1);
    border: 1px solid var(--bg-3);
    border-radius: var(--radius-md);
    padding: 12px 0;
    font-family: var(--mono);
    font-size: 12.5px;
    line-height: 1.55;
    margin: 0;
    overflow-x: auto;
  }

  .line {
    display: block;
    padding: 0 12px;
  }

  .line.highlight {
    background: color-mix(in srgb, var(--accent-2) 18%, transparent);
    border-left: 3px solid var(--accent-2);
    padding-left: 9px;
  }

  .line.wt-highlight {
    background: color-mix(in srgb, var(--warn) 30%, transparent);
    border-left: 3px solid var(--warn);
    padding-left: 9px;
  }

  .lineno {
    display: inline-block;
    width: 36px;
    color: var(--fg-2);
    text-align: right;
    margin-right: 12px;
    user-select: none;
  }

  .content {
    white-space: pre;
  }
</style>
