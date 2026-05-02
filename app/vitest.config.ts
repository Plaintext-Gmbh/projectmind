import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    environment: 'happy-dom',
    include: ['src/**/*.test.ts'],
    setupFiles: ['./src/test-setup.ts'],
    // Each test file gets a fresh module graph so the in-module
    // `inner` writable state from one test never leaks into the next.
    isolate: true,
  },
});
