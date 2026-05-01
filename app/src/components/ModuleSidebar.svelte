<script lang="ts">
  import { modules, moduleFilter } from '../lib/store';
  import { t } from '../lib/i18n';

  function setModule(id: string | null) {
    moduleFilter.update((cur) => (cur === id ? null : id));
  }

  function shortName(id: string): string {
    const idx = id.lastIndexOf(':');
    return idx >= 0 ? id.slice(idx + 1) : id;
  }
</script>

<aside>
  <header>
    <h3>{$t('modules.title')}</h3>
    <span class="count">{$modules.length}</span>
  </header>
  <ul>
    <li class:active={$moduleFilter === null}>
      <button on:click={() => setModule(null)}>
        <span class="name">{$t('modules.all')}</span>
        <span class="badge total">
          {$modules.reduce((acc, m) => acc + m.classes, 0)}
        </span>
      </button>
    </li>
    {#each $modules as m (m.id)}
      <li class:active={$moduleFilter === m.id}>
        <button on:click={() => setModule(m.id)}>
          <span class="name">{shortName(m.id)}</span>
          <span class="badge">{m.classes}</span>
        </button>
      </li>
    {/each}
  </ul>
</aside>

<style>
  aside {
    background: var(--bg-1);
    border-right: 1px solid var(--bg-3);
    overflow: hidden;
    display: flex;
    flex-direction: column;
    width: 220px;
    flex-shrink: 0;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 12px;
    border-bottom: 1px solid var(--bg-3);
  }

  h3 {
    margin: 0;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--fg-2);
    font-weight: 600;
  }

  .count {
    color: var(--fg-2);
    font-family: var(--mono);
    font-size: 11px;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    flex: 1;
  }

  li.active button {
    background: color-mix(in srgb, var(--accent-2) 18%, var(--bg-1));
    border-left: 3px solid var(--accent-2);
    padding-left: 9px;
  }

  button {
    display: flex;
    width: 100%;
    align-items: center;
    justify-content: space-between;
    padding: 6px 12px;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--bg-2);
    color: var(--fg-1);
    text-align: left;
    cursor: pointer;
    font-size: 12px;
    border-radius: 0;
  }

  button:hover {
    background: var(--bg-2);
  }

  .name {
    font-family: var(--mono);
    color: var(--fg-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin-right: 8px;
  }

  .badge {
    font-family: var(--mono);
    color: var(--fg-2);
    font-size: 11px;
    background: var(--bg-2);
    padding: 1px 6px;
    border-radius: 3px;
  }

  .badge.total {
    color: var(--accent);
  }
</style>
