<script lang="ts">
  /// Side rail for the diff view (#126). One marker per hunk, grouped by
  /// file. Clicking a marker asks the parent to scroll+pulse that hunk;
  /// the marker matching the currently focused flat-line index is shown as
  /// active. Pure presentation — the parent owns the scroll/pulse logic
  /// (reuses `focusLineIndex`), so the rail and the tour focus stay in
  /// sync through a single code path.
  import { createEventDispatcher } from 'svelte';
  import { t } from '../lib/i18n';
  import type { DiffFile } from '../lib/diffFocus';

  export let files: DiffFile[] = [];
  /// Flat-line index of the hunk the diff is currently focused on, or
  /// `null` when the diff sits at its natural position. Drives the active
  /// marker so the rail mirrors tour focus + click state.
  export let activeLine: number | null = null;

  const dispatch = createEventDispatcher<{ jump: { startLine: number } }>();

  function basename(p: string): string {
    const parts = p.split(/[\\/]/).filter(Boolean);
    return parts.length ? parts[parts.length - 1] : p;
  }

  /// A single glyph summarising a hunk's magnitude, so the rail reads at a
  /// glance without numbers: additions vs deletions dominance.
  function magnitude(adds: number, dels: number): string {
    if (adds > 0 && dels === 0) return '+';
    if (dels > 0 && adds === 0) return '−';
    return '±';
  }

  $: totalHunks = files.reduce((n, f) => n + f.hunks.length, 0);
</script>

{#if totalHunks > 0}
  <nav class="rail" aria-label={$t('diff.rail.aria')}>
    <ol class="rail-files">
      {#each files as file (file.startLine)}
        {#if file.hunks.length > 0}
          <li class="rail-file">
            <button
              type="button"
              class="rail-file-head"
              title={file.newPath}
              on:click={() => dispatch('jump', { startLine: file.startLine })}
            >
              {basename(file.newPath)}
            </button>
            <ol class="rail-hunks">
              {#each file.hunks as hunk (hunk.startLine)}
                <li>
                  <button
                    type="button"
                    class="rail-marker"
                    class:active={activeLine === hunk.startLine}
                    aria-current={activeLine === hunk.startLine ? 'true' : undefined}
                    title={hunk.header}
                    on:click={() => dispatch('jump', { startLine: hunk.startLine })}
                  >
                    <span class="rail-glyph" aria-hidden="true">{magnitude(hunk.adds, hunk.dels)}</span>
                    <span class="rail-counts">
                      {#if hunk.adds > 0}<span class="rail-add">+{hunk.adds}</span>{/if}
                      {#if hunk.dels > 0}<span class="rail-del">−{hunk.dels}</span>{/if}
                    </span>
                  </button>
                </li>
              {/each}
            </ol>
          </li>
        {/if}
      {/each}
    </ol>
  </nav>
{/if}

<style>
  .rail {
    flex: 0 0 auto;
    width: 132px;
    overflow-y: auto;
    padding: 8px 6px;
    border-left: 1px solid var(--bg-3);
    background: var(--bg-1);
    font-size: 0.72em;
  }
  .rail-files,
  .rail-hunks {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .rail-file + .rail-file {
    margin-top: 8px;
  }
  .rail-file-head {
    display: block;
    width: 100%;
    text-align: left;
    padding: 2px 4px;
    border: 0;
    background: transparent;
    color: var(--fg-1);
    font-family: var(--mono);
    font-weight: 600;
    font-size: inherit;
    cursor: pointer;
    border-radius: 3px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rail-file-head:hover {
    background: var(--bg-2);
  }
  .rail-hunks {
    margin-top: 2px;
  }
  .rail-marker {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 2px 4px 2px 10px;
    border: 0;
    border-left: 3px solid var(--bg-3);
    background: transparent;
    color: var(--fg-2);
    font-family: var(--mono);
    font-size: inherit;
    cursor: pointer;
    text-align: left;
  }
  .rail-marker:hover {
    background: var(--bg-2);
    color: var(--fg-1);
  }
  .rail-marker.active {
    border-left-color: var(--accent-2);
    background: color-mix(in srgb, var(--accent-2) 14%, transparent);
    color: var(--fg-0);
  }
  .rail-glyph {
    width: 0.9em;
    text-align: center;
    color: var(--accent);
    font-weight: 700;
  }
  .rail-counts {
    display: inline-flex;
    gap: 4px;
  }
  .rail-add {
    color: var(--diff-add-fg, #1a7f37);
  }
  .rail-del {
    color: var(--diff-del-fg, #cf222e);
  }
</style>
