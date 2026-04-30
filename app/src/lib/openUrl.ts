/// Open a URL in the system browser. In Tauri-mode the IPC `open_external`
/// command bridges to the Rust `open` crate; in browser-mode we fall back to
/// `window.open` which the host browser already routes correctly.
import { isTauriRuntime } from './api';
import { invoke } from '@tauri-apps/api/core';

export async function openUrl(url: string): Promise<void> {
  if (isTauriRuntime()) {
    try {
      await invoke('open_external', { url });
      return;
    } catch (err) {
      // Fall through to window.open — better something than nothing.
      console.error('open_external failed', err);
    }
  }
  try {
    window.open(url, '_blank', 'noopener,noreferrer');
  } catch (err) {
    console.error('window.open failed', err);
  }
}
