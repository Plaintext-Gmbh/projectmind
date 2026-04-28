import { writable, derived } from 'svelte/store';
import type { ClassEntry, ModuleEntry, RepoSummary } from './api';

export const repo = writable<RepoSummary | null>(null);
export const modules = writable<ModuleEntry[]>([]);
export const classes = writable<ClassEntry[]>([]);
export const selectedClass = writable<ClassEntry | null>(null);
export const stereotypeFilter = writable<string | null>(null);
export const moduleFilter = writable<string | null>(null);
export const errorMessage = writable<string | null>(null);

export const filteredClasses = derived(
  [classes, stereotypeFilter, moduleFilter],
  ([$classes, $stereo, $mod]) =>
    $classes.filter(
      (c) =>
        ($stereo === null || c.stereotypes.includes($stereo)) &&
        ($mod === null || c.module === $mod),
    ),
);

export const stereotypeCounts = derived([classes, moduleFilter], ([$classes, $mod]) => {
  const filtered = $mod === null ? $classes : $classes.filter((c) => c.module === $mod);
  const counts: Record<string, number> = {};
  for (const c of filtered) {
    for (const s of c.stereotypes) {
      counts[s] = (counts[s] || 0) + 1;
    }
  }
  return counts;
});
