// Presenter TTS narrator wiring (Cockpit 2.6, #162).
//
// Two runtimes, one interface:
//   * Desktop (Tauri): call the Rust `speak` command, which spawns the OS
//     synthesiser (`say` on macOS, `espeak` on Linux). Missing binary → a
//     hint string, never a throw.
//   * Browser (iPad / phone via open_browser_repo): use the Web Speech API
//     (`window.speechSynthesis`), which runs on the *client* device — the
//     right place for audio when the server is elsewhere on the LAN.
//
// The narration text can be either the step's authored `narration` or, when
// an MCP client has generated richer prose, whatever text that client
// supplied — this module just speaks the string it is handed (see the PR for
// the LLM-narration wiring notes).

import { invoke } from '@tauri-apps/api/core';
import { isTauriRuntime } from './api';

/** Strip markdown noise so the synthesiser reads clean prose. */
export function narrationToSpeech(markdown: string): string {
  return (
    markdown
      // Fenced code blocks — don't read code aloud.
      .replace(/```[\s\S]*?```/g, ' ')
      // Inline code — keep the content, drop the backticks.
      .replace(/`([^`]*)`/g, '$1')
      // Links: keep the label, drop the URL. `[label](url)` → `label`.
      .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
      // Inline emphasis markers (`*`, `_`) sit flush against words, so drop
      // them with no replacement to avoid stray spaces (`_italic_.` → `italic.`).
      .replace(/[*_]/g, '')
      // Block markers (`#` headings, `>` blockquotes, list bullets) become a
      // space so the following word doesn't fuse onto the previous line.
      .replace(/[#>]+/g, ' ')
      // Collapse whitespace.
      .replace(/\s+/g, ' ')
      .trim()
  );
}

/** Whether TTS can run at all in the current runtime. */
export function speechAvailable(): boolean {
  if (isTauriRuntime()) return true; // desktop resolves availability in Rust
  return typeof window !== 'undefined' && 'speechSynthesis' in window;
}

/**
 * Speak `markdown` narration aloud. Cancels any in-flight utterance first so
 * stepping quickly through a tour doesn't stack voices. Best-effort: any
 * failure is swallowed (returns `false`) — the narrator is a nicety, never a
 * blocker.
 */
export async function speak(markdown: string): Promise<boolean> {
  const text = narrationToSpeech(markdown);
  if (!text) return false;
  try {
    if (isTauriRuntime()) {
      await invoke<string>('speak', { text });
      return true;
    }
    if (typeof window !== 'undefined' && 'speechSynthesis' in window) {
      window.speechSynthesis.cancel();
      const utter = new SpeechSynthesisUtterance(text);
      window.speechSynthesis.speak(utter);
      return true;
    }
  } catch {
    // Desktop command missing or Web Speech unavailable — silent no-op.
  }
  return false;
}

/** Stop any in-flight speech (browser only; the OS commands are fire-and-forget). */
export function stopSpeaking(): void {
  if (!isTauriRuntime() && typeof window !== 'undefined' && 'speechSynthesis' in window) {
    window.speechSynthesis.cancel();
  }
}
