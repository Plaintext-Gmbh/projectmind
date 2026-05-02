<script lang="ts">
  import {
    repo,
    moduleFilter,
    packageFilter,
    stereotypeFilter,
    fileKindFilter,
    walkthroughCursor,
    followingMcp,
    viewMode,
  } from '../lib/store';
  import { history } from '../lib/navigation';
  import { t } from '../lib/i18n';

  function basename(p: string): string {
    const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
    return idx === -1 ? p : p.slice(idx + 1);
  }

  /// "filter chip" rows shown in the middle of the status bar. Empty state
  /// → no chips, no separators. Each chip has an inline × that calls back
  /// into the store to clear that one axis without touching the others.
  $: filters = [
    { kind: 'module' as const, value: $moduleFilter, clear: () => moduleFilter.set(null) },
    { kind: 'package' as const, value: $packageFilter, clear: () => packageFilter.set(null) },
    { kind: 'stereotype' as const, value: $stereotypeFilter, clear: () => stereotypeFilter.set(null) },
    { kind: 'kind' as const, value: $fileKindFilter, clear: () => fileKindFilter.set(null) },
  ].filter((f) => f.value);
</script>

<footer class="status-bar">
  <!-- Left: repo identity + counts -->
  <div class="left">
    {#if $repo}
      <span class="repo" title={$repo.root}>
        <span class="dot"></span>
        <span class="name">{basename($repo.root)}</span>
      </span>
      <span class="sep" aria-hidden="true">·</span>
      <span class="counts">
        {$t('status.repoCount', {
          files: $repo.classes,
          filesUnit: $t($repo.classes === 1 ? 'status.files.one' : 'status.files.other'),
          modules: $repo.modules,
          modulesUnit: $t($repo.modules === 1 ? 'status.modules.one' : 'status.modules.other'),
        })}
      </span>
    {:else}
      <span class="repo idle">
        <span class="dot dim"></span>
        <span class="name">{$t('status.noRepo')}</span>
      </span>
    {/if}
  </div>

  <!-- Middle: active filter chips, click × to clear -->
  <div class="middle">
    {#if filters.length > 0}
      {#each filters as f (f.kind)}
        <button
          class="chip {f.kind}"
          on:click={f.clear}
          title="Clear {f.kind} filter"
          aria-label="Clear {f.kind} filter: {f.value}"
        >
          <span class="chip-label">{f.kind}</span>
          <code class="chip-value">{f.value}</code>
          <span class="chip-x" aria-hidden="true">×</span>
        </button>
      {/each}
    {/if}
  </div>

  <!-- Right: ambient state — view, walkthrough position, MCP follow, history -->
  <div class="right">
    {#if $walkthroughCursor}
      <span class="badge wt" title={$t('status.walkthroughTitle') || 'Active walkthrough'}>
        ▶ step {$walkthroughCursor.step + 1}
      </span>
    {/if}
    {#if $followingMcp}
      <span class="badge mcp" title={$t('status.followingMcpTitle') || 'GUI is following MCP intents'}>
        {$t('status.followingMcp') || 'MCP'}
      </span>
    {/if}
    <span class="view" title="View mode">{$viewMode}</span>
    {#if $history.entries.length > 0}
      <span class="hist" title="History position">
        {$history.cursor + 1}/{$history.entries.length}
      </span>
    {/if}
  </div>
</footer>

<style>
  .status-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 26px;
    padding: 0 12px;
    background: var(--bg-1);
    border-top: 1px solid var(--bg-3);
    font-size: 11px;
    color: var(--fg-2);
    flex-shrink: 0;
    overflow: hidden;
  }

  .left,
  .middle,
  .right {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .left {
    flex-shrink: 0;
  }

  .middle {
    flex: 1;
    min-width: 0;
    overflow-x: auto;
    overflow-y: hidden;
    /* hide horizontal scrollbar but keep wheel-scrolling functional */
    scrollbar-width: none;
  }
  .middle::-webkit-scrollbar {
    display: none;
  }

  .right {
    flex-shrink: 0;
    color: var(--fg-2);
  }

  .repo {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .repo .name {
    color: var(--fg-1);
    font-family: var(--mono);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent-2);
    box-shadow: 0 0 4px color-mix(in srgb, var(--accent-2) 60%, transparent);
  }
  .dot.dim {
    background: var(--fg-2);
    box-shadow: none;
    opacity: 0.4;
  }
  .repo.idle .name {
    color: var(--fg-2);
    font-style: italic;
  }

  .sep {
    color: var(--fg-2);
    opacity: 0.5;
  }

  .counts {
    font-family: var(--mono);
    font-size: 10.5px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 18px;
    padding: 0 6px 0 8px;
    background: color-mix(in srgb, var(--accent-2) 12%, var(--bg-2));
    border: 1px solid color-mix(in srgb, var(--accent-2) 35%, var(--bg-3));
    border-radius: 9px;
    color: var(--fg-1);
    font: inherit;
    font-size: 10.5px;
    cursor: pointer;
    flex-shrink: 0;
  }
  .chip:hover {
    background: color-mix(in srgb, var(--accent-2) 22%, var(--bg-2));
  }
  .chip-label {
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--fg-2);
    font-size: 9px;
  }
  .chip-value {
    font-family: var(--mono);
    color: var(--fg-0);
    max-width: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip-x {
    color: var(--fg-2);
    margin-left: 2px;
    font-size: 12px;
    line-height: 1;
  }
  .chip:hover .chip-x {
    color: var(--accent-2);
  }

  .badge {
    display: inline-flex;
    align-items: center;
    height: 18px;
    padding: 0 8px;
    border-radius: 9px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.03em;
  }
  .badge.wt {
    background: color-mix(in srgb, var(--warn) 18%, var(--bg-2));
    color: var(--warn);
    border: 1px solid color-mix(in srgb, var(--warn) 35%, transparent);
  }
  .badge.mcp {
    background: color-mix(in srgb, var(--accent) 18%, var(--bg-2));
    color: var(--accent);
    border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .view,
  .hist {
    font-family: var(--mono);
    font-size: 10.5px;
    color: var(--fg-2);
  }
  .view {
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .hist {
    min-width: 38px;
    text-align: right;
  }
</style>
