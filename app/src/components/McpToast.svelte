<script lang="ts">
  /// Transparency toast for MCP-driven view changes.
  ///
  /// When an MCP-aware client (Claude Code, ChatGPT, …) calls something
  /// like `view_file` or `view_class`, the GUI quietly switches to that
  /// view. The user can be left wondering "did Claude just do that?" —
  /// especially when several actions land in a row. This toast surfaces
  /// each MCP intent for 4 seconds at the bottom-right corner, with an
  /// inline "stop following" button that cuts the GUI loose from the MCP
  /// stream until the user explicitly re-engages.
  ///
  /// It's deliberately read-only: it does NOT block the view change.
  /// Repo-scoping (`file_access::canonical_file_in_repo`) already
  /// prevents the MCP from reaching files outside the open repo, so a
  /// hard approval gate would be friction without a security win. A
  /// dedicated approval workflow lives on the future-work list.

  import { onMount, onDestroy } from 'svelte';
  import {
    followingMcp,
    fileView,
    viewMode,
    selectedClass,
    walkthroughCursor,
  } from '../lib/store';
  import { t } from '../lib/i18n';

  type Toast = {
    id: number;
    label: string;
    detail: string;
    expiresAt: number;
  };

  const SHOW_FOR_MS = 4000;

  let toasts: Toast[] = [];
  let nextId = 1;
  let interval: ReturnType<typeof setInterval> | null = null;

  function basename(p: string | null | undefined): string {
    if (!p) return '';
    const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
    return idx === -1 ? p : p.slice(idx + 1);
  }

  function emit(label: string, detail: string) {
    toasts = [
      ...toasts.slice(-2), // keep at most 3 visible at a time
      { id: nextId++, label, detail, expiresAt: Date.now() + SHOW_FOR_MS },
    ];
  }

  function dismiss(id: number) {
    toasts = toasts.filter((t) => t.id !== id);
  }

  function stopFollowing() {
    followingMcp.set(false);
    toasts = [];
  }

  // Whenever an MCP-driven view change lands, drop a toast. The watchers
  // only fire while followingMcp is true; the user clicking any tab or
  // the "stop following" button flips that flag and silences us.
  let lastFile: string | null = null;
  let lastClass: string | null = null;
  let lastView: string | null = null;
  let lastWalkthroughId: string | null = null;
  $: if ($followingMcp && $fileView && $fileView.path !== lastFile) {
    lastFile = $fileView.path;
    emit($t('mcp.toast.file') || 'MCP opened file', basename($fileView.path));
  }
  $: if ($followingMcp && $selectedClass && $selectedClass.fqn !== lastClass) {
    lastClass = $selectedClass.fqn;
    emit($t('mcp.toast.class') || 'MCP opened class', $selectedClass.fqn);
  }
  $: if ($followingMcp && $viewMode !== lastView && $viewMode !== 'classes') {
    lastView = $viewMode;
    emit($t('mcp.toast.view') || 'MCP switched view', $viewMode);
  }
  // Walkthroughs deserve their own toast: the user might be on another
  // tab or in another app entirely when an LLM starts a tour, and a
  // generic "view changed" message buries the lede. We watch the cursor
  // id (not just `viewMode === 'walkthrough'`) so re-running an already-
  // open tour also surfaces — same id is suppressed but a new id wins.
  $: if (
    $followingMcp &&
    $walkthroughCursor &&
    $walkthroughCursor.id !== lastWalkthroughId
  ) {
    lastWalkthroughId = $walkthroughCursor.id;
    emit(
      $t('mcp.toast.walkthrough') || 'MCP started tour',
      $walkthroughCursor.id,
    );
    flashTitleIfHidden();
  }

  // ----- Background notification ------------------------------------------

  /// When a tour starts and the window is hidden / blurred, prepend "▶ "
  /// to `document.title` so the user sees something change in their tab
  /// strip / dock. Restore the original title as soon as the window
  /// regains focus or visibility — same heuristic as web chat apps.
  let originalTitle: string | null = null;

  function flashTitleIfHidden() {
    if (typeof document === 'undefined') return;
    if (!document.hidden && document.hasFocus()) return;
    if (originalTitle === null) originalTitle = document.title;
    if (!document.title.startsWith('▶ ')) {
      document.title = `▶ ${originalTitle}`;
    }
  }

  function restoreTitle() {
    if (originalTitle === null) return;
    if (typeof document !== 'undefined') {
      document.title = originalTitle;
    }
    originalTitle = null;
  }

  onMount(() => {
    interval = setInterval(() => {
      const now = Date.now();
      const next = toasts.filter((t) => t.expiresAt > now);
      if (next.length !== toasts.length) toasts = next;
    }, 500);
    if (typeof window !== 'undefined') {
      window.addEventListener('focus', restoreTitle);
      window.addEventListener('visibilitychange', restoreTitle);
    }
  });
  onDestroy(() => {
    if (interval) clearInterval(interval);
    if (typeof window !== 'undefined') {
      window.removeEventListener('focus', restoreTitle);
      window.removeEventListener('visibilitychange', restoreTitle);
    }
    // If we still have a flashed title, put the original back so the next
    // mount of the component (or the test harness) doesn't see a stale "▶".
    restoreTitle();
  });
</script>

{#if toasts.length > 0}
  <aside class="stack" role="status" aria-live="polite">
    {#each toasts as toast (toast.id)}
      <div class="toast">
        <span class="badge">MCP</span>
        <div class="text">
          <span class="label">{toast.label}</span>
          <code class="detail">{toast.detail}</code>
        </div>
        <button
          class="dismiss"
          on:click={() => dismiss(toast.id)}
          title={$t('keyboard.row.close') || 'Dismiss'}
          aria-label="Dismiss"
        >×</button>
      </div>
    {/each}
    <button class="stop" on:click={stopFollowing}>
      {$t('mcp.toast.stop') || 'Stop following MCP'}
    </button>
  </aside>
{/if}

<style>
  .stack {
    position: fixed;
    right: 16px;
    bottom: 40px; /* sit above the status bar */
    display: flex;
    flex-direction: column;
    gap: 6px;
    z-index: 800;
    max-width: 360px;
  }
  .toast {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    background: var(--bg-1);
    border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--bg-3));
    border-left: 3px solid var(--accent);
    border-radius: 6px;
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.4);
    font-size: 12px;
    color: var(--fg-1);
    animation: slide-in 180ms ease-out;
  }
  .badge {
    background: color-mix(in srgb, var(--accent) 20%, var(--bg-2));
    color: var(--accent);
    border-radius: 4px;
    padding: 2px 6px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.05em;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .label {
    color: var(--fg-2);
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .detail {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--fg-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dismiss {
    background: transparent;
    border: 0;
    color: var(--fg-2);
    cursor: pointer;
    font-size: 14px;
    line-height: 1;
    padding: 0 2px;
  }
  .dismiss:hover {
    color: var(--accent);
  }
  .stop {
    align-self: flex-end;
    background: var(--bg-2);
    border: 1px solid var(--bg-3);
    color: var(--fg-1);
    padding: 4px 10px;
    border-radius: 4px;
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }
  .stop:hover {
    border-color: var(--accent-2);
    color: var(--accent-2);
  }
  @keyframes slide-in {
    from {
      transform: translateX(100%);
      opacity: 0;
    }
    to {
      transform: translateX(0);
      opacity: 1;
    }
  }
</style>
