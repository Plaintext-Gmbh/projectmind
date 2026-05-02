/// Vitest setup. Node 25 ships an experimental built-in `localStorage`
/// without `clear()`, which clashes with happy-dom's polyfill (and with
/// the contract our app code expects). We replace it with a hand-rolled
/// in-memory shim before any test starts so every test sees the same
/// well-behaved API.

import { beforeEach, vi } from 'vitest';

class MemoryStorage implements Storage {
  private store = new Map<string, string>();
  get length() {
    return this.store.size;
  }
  clear() {
    this.store.clear();
  }
  getItem(key: string): string | null {
    return this.store.has(key) ? (this.store.get(key) as string) : null;
  }
  key(index: number): string | null {
    return Array.from(this.store.keys())[index] ?? null;
  }
  removeItem(key: string) {
    this.store.delete(key);
  }
  setItem(key: string, value: string) {
    this.store.set(key, String(value));
  }
}

vi.stubGlobal('localStorage', new MemoryStorage());

beforeEach(() => {
  (globalThis.localStorage as Storage).clear();
});
