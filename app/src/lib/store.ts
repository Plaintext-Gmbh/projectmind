import { writable, derived } from 'svelte/store';
import type { ClassEntry, RepoSummary } from './api';

export const repo = writable<RepoSummary | null>(null);
export const classes = writable<ClassEntry[]>([]);
export const selectedClass = writable<ClassEntry | null>(null);
export const stereotypeFilter = writable<string | null>(null);
export const errorMessage = writable<string | null>(null);

export const filteredClasses = derived(
  [classes, stereotypeFilter],
  ([$classes, $filter]) =>
    $filter ? $classes.filter((c) => c.stereotypes.includes($filter)) : $classes,
);

export const stereotypeCounts = derived(classes, ($classes) => {
  const counts: Record<string, number> = {};
  for (const c of $classes) {
    for (const s of c.stereotypes) {
      counts[s] = (counts[s] || 0) + 1;
    }
  }
  return counts;
});
