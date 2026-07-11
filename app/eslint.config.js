// ESLint 9 flat config for the Svelte 5 + TypeScript frontend.
//
// Layers:
//   1. typescript-eslint flat/recommended — registers the TS parser + plugin
//      and the recommended rule set (applied to *.ts and *.svelte).
//   2. eslint-plugin-svelte flat/recommended — svelte-eslint-parser for
//      *.svelte plus the recommended Svelte rules.
//   3. A *.svelte override wiring @typescript-eslint/parser into the Svelte
//      parser so <script lang="ts"> blocks get the TS rules too.
import tsPlugin from '@typescript-eslint/eslint-plugin';
import tsParser from '@typescript-eslint/parser';
import svelte from 'eslint-plugin-svelte';

export default [
  {
    // Build output and the Rust/Tauri shell are not lint targets.
    ignores: ['dist/', 'src-tauri/'],
  },
  ...tsPlugin.configs['flat/recommended'],
  ...svelte.configs['flat/recommended'],
  {
    files: ['**/*.svelte'],
    languageOptions: {
      parserOptions: {
        // Parse <script lang="ts"> blocks with the TypeScript parser.
        parser: tsParser,
      },
    },
  },
  {
    rules: {
      // `_`-prefixed bindings are the repo's deliberate placeholder
      // convention (e.g. reactive-statement triggers, ignored tuple parts).
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
      // {@html} is the core rendering path of this app: the pure diagram
      // renderers in src/lib/diagrams/*.ts return SVG strings (all dynamic
      // text goes through their esc() helper) that the stage components
      // mount verbatim. No user-supplied content flows into these strings,
      // so the XSS guard would only produce a mandatory suppression on
      // every renderer mount.
      'svelte/no-at-html-tags': 'off',
    },
  },
];
