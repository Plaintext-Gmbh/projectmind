/// Cache-aware dynamic-import helper for Svelte components.
///
/// Svelte's `{#await}` block reruns the loader expression on every render
/// pass, so we cache the resulting Promise per loader-id. Inside templates:
///
///     {#await loadComponent('DiagramView', () => import('./components/DiagramView.svelte')) then m}
///       <svelte:component this={m.default} … />
///     {/await}
///
/// Every subsequent render reuses the cached Promise. Once the import has
/// resolved the Promise is stored as already-resolved, so the await block
/// renders synchronously after the first time.

const cache = new Map<string, Promise<{ default: unknown }>>();

export function loadComponent<T>(
  id: string,
  loader: () => Promise<{ default: T }>,
): Promise<{ default: T }> {
  let p = cache.get(id) as Promise<{ default: T }> | undefined;
  if (!p) {
    p = loader();
    cache.set(id, p as Promise<{ default: unknown }>);
  }
  return p;
}
