<script lang="ts">
  import DrawIoFrame from './DrawIoFrame.svelte';
  import { readFileText, saveDrawio } from '../lib/api';
  import { createShiftWheelZoom } from '../lib/shiftWheelZoom';
  import { savedAtLabel } from '../lib/drawioEmbed';
  import { t } from '../lib/i18n';

  export let path: string;

  let xml = '';
  let loading = false;
  let error: string | null = null;

  // Edit mode (#142, draw.io edit path): the embedded diagrams.net editor
  // replaces the read-only canvas; its Save button hands the XML back and we
  // write it into the repo through the host (Tauri command / browser-host
  // route → core `save_drawio`, which enforces the repo boundary). Exit
  // returns to the viewer and re-reads the file from disk, so what you see
  // is always what is saved.
  let editing = false;
  let frame: DrawIoFrame | undefined;
  let saveState: 'idle' | 'saving' | 'saved' | 'error' = 'idle';
  let savedAt = '';
  let saveError = '';

  const { zoom, action: zoomAction } = createShiftWheelZoom('projectmind.drawio.zoom');

  $: void load(path);
  // A different file always starts in view mode.
  $: if (path) {
    editing = false;
    saveState = 'idle';
  }

  $: fileName = path.split(/[\\/]/).pop() ?? path;

  async function load(p: string) {
    if (!p) return;
    loading = true;
    error = null;
    try {
      xml = await readFileText(p);
    } catch (e) {
      error = String(e);
      xml = '';
    } finally {
      loading = false;
    }
  }

  function startEditing() {
    saveState = 'idle';
    editing = true;
  }

  async function onSave(ev: CustomEvent<string>) {
    saveState = 'saving';
    try {
      await saveDrawio(path, ev.detail);
      xml = ev.detail;
      savedAt = savedAtLabel(new Date());
      saveState = 'saved';
      frame?.acknowledgeSaved();
    } catch (e) {
      saveError = String(e);
      saveState = 'error';
    }
  }

  async function onExit() {
    editing = false;
    // Whatever was saved is on disk; unsaved edits are dropped by design —
    // the editor's own Save is the single write path.
    await load(path);
  }
</script>

<section class="root" use:zoomAction style="font-size: {$zoom}em;">
  <header class="bar">
    <span class="kind">drawio</span>
    <span class="path">{path}</span>
    <span class="spacer"></span>
    {#if editing}
      <span class="hint">{$t('drawio.editHint')}</span>
      {#if saveState === 'saving'}
        <span class="status">{$t('drawio.saving')}</span>
      {:else if saveState === 'saved'}
        <span class="status ok">{$t('drawio.saved', { time: savedAt })}</span>
      {:else if saveState === 'error'}
        <span class="status err">{$t('drawio.saveFailed', { error: saveError })}</span>
      {/if}
      <button type="button" class="mode" on:click={onExit}>{$t('drawio.done')}</button>
    {:else}
      <button
        type="button"
        class="mode"
        on:click={startEditing}
        disabled={loading || !!error}
        title={$t('drawio.edit.tooltip')}
      >
        {$t('drawio.edit')}
      </button>
    {/if}
  </header>
  <div class="body">
    {#if loading}
      <div class="empty">Loading…</div>
    {:else if error}
      <div class="error">⚠ {error}</div>
    {:else}
      <DrawIoFrame
        bind:this={frame}
        {xml}
        title={fileName}
        editable={editing}
        on:save={onSave}
        on:exit={onExit}
      />
    {/if}
  </div>
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
    flex-shrink: 0;
  }
  .kind {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 2px 6px;
    background: var(--bg-2);
    border-radius: 3px;
    color: var(--fg-2);
  }
  .path {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--fg-1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
  .hint {
    font-size: 11px;
    color: var(--fg-2);
    white-space: nowrap;
  }
  .status {
    font-size: 11px;
    color: var(--fg-2);
    white-space: nowrap;
  }
  .status.ok {
    color: var(--ok, #3fb950);
  }
  .status.err {
    color: var(--error);
  }
  .mode {
    font-size: 12px;
    padding: 3px 10px;
    border-radius: 4px;
    border: 1px solid var(--bg-3);
    background: var(--bg-2);
    color: var(--fg-1);
    cursor: pointer;
    white-space: nowrap;
  }
  .mode:hover:not(:disabled) {
    background: var(--bg-3);
  }
  .mode:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    overflow: hidden;
  }
  .empty,
  .error {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--fg-2);
    font-size: 13px;
  }
  .error {
    color: var(--error);
  }
</style>
