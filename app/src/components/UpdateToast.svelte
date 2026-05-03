<script lang="ts">
  // Surface available app updates as a non-blocking toast in the lower-right
  // corner. Click "Update" to download + install + relaunch; click × to
  // dismiss until the next background check fires.
  import { updateState, applyUpdate, dismissUpdate } from '../lib/updater';
  import { t } from '../lib/i18n';
</script>

{#if $updateState.kind === 'available'}
  <div class="update-toast" role="status" aria-live="polite">
    <div class="update-text">
      <strong>{$t('updater.available', { version: $updateState.version })}</strong>
      {#if $updateState.notes}
        <span class="update-notes">{$updateState.notes}</span>
      {/if}
    </div>
    <button class="update-action" on:click={() => applyUpdate($updateState.update)}>
      {$t('updater.install')}
    </button>
    <button class="update-dismiss" on:click={dismissUpdate} aria-label={$t('updater.dismiss')}>×</button>
  </div>
{:else if $updateState.kind === 'installing'}
  <div class="update-toast" role="status" aria-live="polite">
    <div class="update-text">
      <strong>{$t('updater.installing', { version: $updateState.version })}</strong>
    </div>
  </div>
{:else if $updateState.kind === 'error'}
  <div class="update-toast error" role="alert">
    <div class="update-text">
      <strong>{$t('updater.error')}</strong>
      <span class="update-notes">{$updateState.message}</span>
    </div>
    <button class="update-dismiss" on:click={dismissUpdate} aria-label={$t('updater.dismiss')}>×</button>
  </div>
{/if}

<style>
  .update-toast {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 1000;
    display: flex;
    align-items: flex-start;
    gap: 12px;
    max-width: 380px;
    padding: 12px 14px;
    background: var(--bg-1);
    border: 1px solid var(--accent-2);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
    color: var(--fg-1);
  }
  .update-toast.error {
    border-color: #d35;
  }
  .update-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 13px;
  }
  .update-notes {
    color: var(--fg-3);
    font-size: 12px;
    white-space: pre-wrap;
    overflow: hidden;
    max-height: 4em;
    text-overflow: ellipsis;
  }
  .update-action {
    background: var(--accent-2);
    color: white;
    border: 1px solid var(--accent-2);
    border-radius: 4px;
    padding: 6px 12px;
    font: inherit;
    font-weight: 500;
    cursor: pointer;
  }
  .update-action:hover {
    background: color-mix(in srgb, var(--accent-2) 80%, white);
  }
  .update-dismiss {
    background: transparent;
    border: none;
    color: var(--fg-3);
    font-size: 18px;
    line-height: 1;
    padding: 0 4px;
    cursor: pointer;
  }
  .update-dismiss:hover {
    color: var(--fg-1);
  }
</style>
