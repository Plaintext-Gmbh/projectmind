// Auto-update via tauri-plugin-updater. The plugin polls
// `releases/latest/download/latest.json`, verifies the signature against the
// public key embedded in tauri.conf.json, and stages the bundle. We don't
// auto-install: surface a toast and let the user trigger the install so they
// don't lose work mid-session.

import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { writable, type Writable } from 'svelte/store';
import { isTauriRuntime } from './api';

export type UpdateState =
  | { kind: 'idle' }
  | { kind: 'available'; version: string; notes: string | null; update: Update }
  | { kind: 'installing'; version: string }
  | { kind: 'error'; message: string };

export const updateState: Writable<UpdateState> = writable({ kind: 'idle' });

/// Probe the updater endpoint. Silent on errors (network down, GitHub rate
/// limit) — we never want a flaky check to surface as a UI error during normal
/// use.
export async function checkForUpdate(): Promise<void> {
  if (!isTauriRuntime()) return;
  try {
    const update = await check();
    if (update) {
      updateState.set({
        kind: 'available',
        version: update.version,
        notes: update.body ?? null,
        update,
      });
    }
  } catch (err) {
    // Silent: don't pester the user about transient network failures. Logged
    // to the JS console via the throw — visible if devtools are open.
    console.warn('update check failed:', err);
  }
}

/// User accepted the update. Download + install + relaunch. If anything goes
/// wrong mid-flight, surface it instead of leaving the user wondering.
export async function applyUpdate(update: Update): Promise<void> {
  updateState.set({ kind: 'installing', version: update.version });
  try {
    await update.downloadAndInstall();
    await relaunch();
  } catch (err) {
    updateState.set({
      kind: 'error',
      message: err instanceof Error ? err.message : String(err),
    });
  }
}

export function dismissUpdate(): void {
  updateState.set({ kind: 'idle' });
}

const SIX_HOURS_MS = 6 * 60 * 60 * 1000;

/// Kick off an initial check 5s after start (let the window paint first), and
/// re-check every 6h while the app stays open.
export function startBackgroundChecks(): void {
  if (!isTauriRuntime()) return;
  setTimeout(() => {
    void checkForUpdate();
  }, 5000);
  setInterval(() => {
    void checkForUpdate();
  }, SIX_HOURS_MS);
}
