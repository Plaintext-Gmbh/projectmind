<script lang="ts">
  import { onMount } from 'svelte';
  import mermaid from 'mermaid';
  import { showDiagram } from '../lib/api';

  export let kind: 'bean-graph' | 'package-tree';

  let container: HTMLDivElement;
  let mermaidSource = '';
  let svg = '';
  let loading = false;
  let error: string | null = null;

  $: if (kind) {
    void render(kind);
  }

  onMount(() => {
    mermaid.initialize({ startOnLoad: false, theme: 'dark', securityLevel: 'loose' });
  });

  async function render(k: 'bean-graph' | 'package-tree') {
    loading = true;
    error = null;
    try {
      mermaidSource = await showDiagram(k);
      const id = `mermaid-${Date.now()}`;
      const result = await mermaid.render(id, mermaidSource);
      svg = result.svg;
    } catch (err) {
      error = String(err);
      svg = '';
    } finally {
      loading = false;
    }
  }
</script>

<div class="root">
  {#if loading}
    <div class="placeholder">Rendering diagram…</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
    <pre>{mermaidSource}</pre>
  {:else}
    <div class="diagram" bind:this={container}>
      {@html svg}
    </div>
  {/if}
</div>

<style>
  .root {
    flex: 1;
    overflow: auto;
    padding: 24px;
    background: var(--bg-0);
  }

  .placeholder {
    color: var(--fg-2);
    text-align: center;
    padding: 40px;
  }

  .error {
    color: var(--error);
    padding: 12px;
    border: 1px solid var(--error);
    border-radius: var(--radius-sm);
  }

  pre {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--fg-1);
    background: var(--bg-1);
    padding: 12px;
    border-radius: var(--radius-sm);
    overflow-x: auto;
  }

  .diagram :global(svg) {
    max-width: 100%;
    height: auto;
    display: block;
    margin: 0 auto;
  }
</style>
