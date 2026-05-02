<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from '../lib/i18n';

  export let open = false;

  // Detect macOS so we render ⌘ on Mac and Ctrl/Alt elsewhere. Tauri's
  // navigator.platform is reliable here.
  const IS_MAC =
    typeof navigator !== 'undefined' && /Mac|iPad|iPhone|iPod/.test(navigator.platform);

  type Section = { title: string; rows: { keys: string[]; desc: string }[] };

  $: sections = [
    {
      title: $t('keyboard.section.navigation'),
      rows: [
        {
          keys: IS_MAC ? ['⌘', '['] : ['Alt', '←'],
          desc: $t('keyboard.row.back'),
        },
        {
          keys: IS_MAC ? ['⌘', ']'] : ['Alt', '→'],
          desc: $t('keyboard.row.forward'),
        },
        {
          keys: ['?'],
          desc: $t('keyboard.row.help'),
        },
        {
          keys: ['Esc'],
          desc: $t('keyboard.row.close'),
        },
      ],
    },
    {
      title: $t('keyboard.section.zoom'),
      rows: [
        {
          keys: ['Shift', '⇕'],
          desc: $t('keyboard.row.zoom'),
        },
      ],
    },
  ] as Section[];

  function close() {
    open = false;
  }

  function onKey(ev: KeyboardEvent) {
    if (!open) return;
    if (ev.key === 'Escape') {
      ev.preventDefault();
      close();
    }
  }

  onMount(() => window.addEventListener('keydown', onKey));
  onDestroy(() => window.removeEventListener('keydown', onKey));
</script>

{#if open}
  <div
    class="backdrop"
    role="presentation"
    on:click={close}
    on:keydown={(e) => e.key === 'Escape' && close()}
  >
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="kbd-help-title"
      tabindex="-1"
      on:click|stopPropagation
      on:keydown|stopPropagation
    >
      <header>
        <h2 id="kbd-help-title">{$t('keyboard.title')}</h2>
        <button class="close" on:click={close} aria-label={$t('keyboard.row.close')}>×</button>
      </header>
      <div class="body">
        {#each sections as section (section.title)}
          <section>
            <h3>{section.title}</h3>
            <dl>
              {#each section.rows as row, i (i)}
                <dt>
                  {#each row.keys as key, j (j)}
                    {#if j > 0}<span class="plus">+</span>{/if}
                    <kbd>{key}</kbd>
                  {/each}
                </dt>
                <dd>{row.desc}</dd>
              {/each}
            </dl>
          </section>
        {/each}
      </div>
      <footer>
        <span class="hint">{$t('keyboard.hint')}</span>
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    backdrop-filter: blur(2px);
  }
  .dialog {
    background: var(--bg-0);
    color: var(--fg-0);
    border: 1px solid var(--bg-3);
    border-radius: 10px;
    box-shadow: 0 20px 50px rgba(0, 0, 0, 0.5);
    width: min(560px, 92vw);
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px;
    border-bottom: 1px solid var(--bg-3);
    background: var(--bg-1);
  }
  header h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  .close {
    background: transparent;
    border: 0;
    color: var(--fg-2);
    font-size: 22px;
    line-height: 1;
    cursor: pointer;
    padding: 0 4px;
  }
  .close:hover {
    color: var(--fg-0);
  }
  .body {
    padding: 16px 20px 8px;
    overflow-y: auto;
  }
  section {
    margin-bottom: 16px;
  }
  section h3 {
    margin: 0 0 8px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--fg-2);
    font-weight: 600;
  }
  dl {
    display: grid;
    grid-template-columns: minmax(120px, max-content) 1fr;
    column-gap: 24px;
    row-gap: 8px;
    margin: 0;
  }
  dt {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  dd {
    margin: 0;
    color: var(--fg-1);
    font-size: 13px;
    align-self: center;
  }
  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 22px;
    height: 22px;
    padding: 0 6px;
    background: var(--bg-2);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-0);
    box-shadow: 0 1px 0 var(--bg-3);
  }
  .plus {
    color: var(--fg-2);
    font-size: 11px;
  }
  footer {
    border-top: 1px solid var(--bg-3);
    padding: 10px 18px;
    background: var(--bg-1);
  }
  .hint {
    font-size: 11px;
    color: var(--fg-2);
  }
</style>
