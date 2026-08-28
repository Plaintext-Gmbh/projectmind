// ESLint 10 flat config for the Svelte 5 + TypeScript frontend.
//
// Layers:
//   1. typescript-eslint flat/recommended — registers the TS parser + plugin
//      and the recommended rule set (applied to *.ts and *.svelte).
//   2. eslint-plugin-svelte recommended (flat is the default since v3) —
//      svelte-eslint-parser for *.svelte plus the recommended Svelte rules.
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
  ...svelte.configs.recommended,
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
      // eslint-plugin-svelte 3 promoted three heuristics into `recommended`
      // that do not fit this (legacy-reactivity, `$:`-based) codebase:
      //
      // - prefer-svelte-reactivity wants SvelteMap/SvelteSet, which only
      //   matter in runes mode ($state). The components here use plain
      //   Map/Set with explicit reassignment for reactivity on purpose.
      // - infinite-reactive-loop flags every `$:` block that calls an async
      //   loader assigning reactive state. The repo's loader convention
      //   (`if (x.fqn !== loadedFqn) { loadedFqn = x.fqn; void load() }`)
      //   guards exactly that — the rule cannot see the guard.
      // - require-each-key: keys must be chosen deliberately here; a
      //   mechanically added non-unique key crashes the block at runtime
      //   (each_key_duplicate, see #171). Left to review, not to the linter.
      'svelte/prefer-svelte-reactivity': 'off',
      'svelte/infinite-reactive-loop': 'off',
      'svelte/require-each-key': 'off',
    },
  },
];
