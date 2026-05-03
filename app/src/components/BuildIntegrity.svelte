<script lang="ts">
  // Build-integrity dialog. Opened via the shield button in the header.
  // Surfaces three things the user might want to verify before trusting the
  // app with anything sensitive:
  //
  //   1. Is this a tagged release built by the CI matrix, or a self-compiled
  //      / forked dev build? (PROJECTMIND_RELEASE_BUILD env var at compile time)
  //   2. Which git commit / build time? (CI passes those through; dev builds
  //      have neither, which is itself a useful signal.)
  //   3. Does the embedded updater public key match the official one? (we
  //      only show the SHA-256 prefix here; the user compares it against the
  //      one printed in the release notes / docs to confirm the channel.)
  //
  // Trust note: this dialog reports markers from the app's own bundle. A
  // tampered binary could lie. The honest answer is "yes, but if you don't
  // trust the binary you can't trust anything it tells you" — the dialog is
  // useful for the casual case of "did I accidentally install a fork" or
  // "am I still running the dev build I cargo-built last Tuesday".
  import { onMount } from 'svelte';
  import { getBuildIntegrity, isTauriRuntime, type BuildIntegrity } from '../lib/api';
  import { t } from '../lib/i18n';

  // The official updater pubkey hash (first 12 hex chars of SHA-256). Update
  // this string when rotating the updater key (see
  // `docs/security/updater-keys.md`). When the running build reports a
  // different prefix we flag it as a non-official channel.
  const OFFICIAL_PUBKEY_PREFIX = '1f13281cd346';

  export let open = false;

  let info: BuildIntegrity | null = null;
  let loaded = false;

  onMount(async () => {
    if (!isTauriRuntime()) {
      loaded = true;
      return;
    }
    try {
      info = await getBuildIntegrity();
    } catch (err) {
      console.warn('build-integrity probe failed:', err);
    } finally {
      loaded = true;
    }
  });

  function close() {
    open = false;
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && open) close();
  }

  $: pubkeyMatches = info ? info.updater_pubkey_short === OFFICIAL_PUBKEY_PREFIX : null;
  $: status = !info
    ? 'unknown'
    : !info.is_release_build
      ? 'dev'
      : pubkeyMatches === false
        ? 'forked'
        : 'official';
</script>

<svelte:window on:keydown={onKeydown} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="integrity-backdrop" on:click={close}></div>
  <div class="integrity-dialog" role="dialog" aria-labelledby="integrity-title">
    <header>
      <h2 id="integrity-title">{$t('integrity.title')}</h2>
      <button class="integrity-close" on:click={close} aria-label={$t('integrity.close')}>×</button>
    </header>

    {#if !loaded}
      <p class="integrity-row">{$t('integrity.loading')}</p>
    {:else if !info}
      <p class="integrity-row integrity-warn">
        {$t('integrity.browser')}
      </p>
    {:else}
      <div class="integrity-status integrity-status-{status}">
        {#if status === 'official'}
          <span class="badge ok">✓</span>
          <strong>{$t('integrity.status.official')}</strong>
          <span class="muted">{$t('integrity.status.official.detail')}</span>
        {:else if status === 'dev'}
          <span class="badge warn">⚠</span>
          <strong>{$t('integrity.status.dev')}</strong>
          <span class="muted">{$t('integrity.status.dev.detail')}</span>
        {:else if status === 'forked'}
          <span class="badge warn">⚠</span>
          <strong>{$t('integrity.status.forked')}</strong>
          <span class="muted">{$t('integrity.status.forked.detail')}</span>
        {/if}
      </div>

      <dl class="integrity-rows">
        <dt>{$t('integrity.field.version')}</dt>
        <dd><code>{info.version}</code></dd>

        <dt>{$t('integrity.field.buildType')}</dt>
        <dd>
          {#if info.is_release_build}
            <code>release</code>
          {:else}
            <code>dev</code>
          {/if}
        </dd>

        <dt>{$t('integrity.field.commit')}</dt>
        <dd>
          {#if info.git_commit}
            <code>{info.git_commit.slice(0, 12)}</code>
          {:else}
            <span class="muted">{$t('integrity.field.commit.unknown')}</span>
          {/if}
        </dd>

        <dt>{$t('integrity.field.builtAt')}</dt>
        <dd>
          {#if info.built_at}
            <code>{info.built_at}</code>
          {:else}
            <span class="muted">{$t('integrity.field.builtAt.unknown')}</span>
          {/if}
        </dd>

        <dt>{$t('integrity.field.updaterKey')}</dt>
        <dd>
          <code>{info.updater_pubkey_short}…</code>
          {#if pubkeyMatches === false}
            <span class="muted"> ≠ {OFFICIAL_PUBKEY_PREFIX}</span>
          {/if}
        </dd>
      </dl>

      <p class="integrity-foot">
        {$t('integrity.foot.docs')}
      </p>
    {/if}
  </div>
{/if}

<style>
  .integrity-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    z-index: 1100;
  }
  .integrity-dialog {
    position: fixed;
    z-index: 1101;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    min-width: 380px;
    max-width: 560px;
    max-height: 80vh;
    overflow: auto;
    background: var(--bg-1);
    border: 1px solid var(--bg-3);
    border-radius: 8px;
    padding: 18px 22px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4);
    color: var(--fg-1);
  }
  .integrity-dialog header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
  }
  .integrity-dialog h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }
  .integrity-close {
    background: transparent;
    border: none;
    color: var(--fg-3);
    font-size: 22px;
    line-height: 1;
    cursor: pointer;
  }
  .integrity-close:hover {
    color: var(--fg-1);
  }

  .integrity-status {
    display: grid;
    grid-template-columns: auto auto 1fr;
    align-items: baseline;
    column-gap: 8px;
    padding: 10px 12px;
    margin-bottom: 14px;
    border-radius: 6px;
    background: var(--bg-2);
  }
  .integrity-status-official {
    background: color-mix(in srgb, #1c8d4f 18%, var(--bg-1));
    border: 1px solid #1c8d4f55;
  }
  .integrity-status-dev,
  .integrity-status-forked {
    background: color-mix(in srgb, #b87a00 18%, var(--bg-1));
    border: 1px solid #b87a0055;
  }
  .integrity-status .badge {
    width: 22px;
    height: 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    font-weight: 600;
    font-size: 13px;
  }
  .integrity-status .badge.ok {
    background: #1c8d4f;
    color: white;
  }
  .integrity-status .badge.warn {
    background: #b87a00;
    color: white;
  }
  .integrity-status .muted {
    grid-column: 3;
    font-size: 12px;
    color: var(--fg-3);
  }
  .integrity-status strong {
    font-size: 14px;
  }

  .integrity-rows {
    display: grid;
    grid-template-columns: max-content 1fr;
    column-gap: 14px;
    row-gap: 6px;
    margin: 0 0 12px 0;
    font-size: 13px;
  }
  .integrity-rows dt {
    color: var(--fg-3);
  }
  .integrity-rows dd {
    margin: 0;
  }
  .integrity-rows code {
    background: var(--bg-2);
    padding: 1px 6px;
    border-radius: 3px;
    font-family: var(--mono, ui-monospace, SFMono-Regular, monospace);
    font-size: 12px;
  }

  .integrity-warn {
    color: var(--fg-3);
  }
  .muted {
    color: var(--fg-3);
  }
  .integrity-foot {
    margin: 8px 0 0 0;
    font-size: 11px;
    color: var(--fg-3);
  }
</style>
