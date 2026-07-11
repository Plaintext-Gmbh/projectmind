/// Unused-key guard, the counterpart of i18nParity.test.ts: parity forces
/// every locale to mirror en.json, so a dead key is not one stale line but
/// five translations that have to be maintained forever. This test forces
/// every en.json key to be referenced somewhere in the app sources
/// (app/src/**/*.svelte|ts, test files excluded — a key that only a test
/// mentions is still dead in the app).
///
/// Detection is deliberately conservative: a key counts as referenced when
/// it appears anywhere in a scanned source file wrapped in quotes or
/// backticks ('key', "key", `key`). That covers direct $t('key') calls as
/// well as keys stored in data structures (e.g. the LEGEND rows in
/// DiagramDiffStep.svelte) and the backend-supplied TabDescriptor
/// label_keys ("nav.files"/"nav.diagrams", both also referenced literally
/// in components). False positives (a quoted key inside a comment) keep a
/// key alive; false negatives cannot delete a used key silently — the
/// failure message tells you to either delete the key or allowlist it.
///
/// DYNAMIC KEY ALLOWLIST — keys built at runtime via template literals are
/// invisible to the literal scan and are derived here from the same value
/// sets the components use (so the allowlist cannot silently drift):
///
///   1. DiagramView.svelte renders the folder-map recency legend with
///      `diagram.colourBy.recency.${stop.key}` where stop.key comes from
///      recencyLegend() in folderMapColors.ts ('today' | 'week' | 'stale').
///
///   2. DiagramDiffStep.svelte renders its mode toggle with
///      `walkthrough.diagramDiff.mode.${m === 'changed-only' ? 'changedOnly' : m}`
///      over DIAGRAM_DIFF_MODES from diagramDiff.ts. The kebab→camel
///      mapping below mirrors that call site.
///
/// Adding a new dynamically constructed key pattern? Extend dynamicKeys
/// below with the derived keys and document the constructing call site.

/// <reference types="vite/client" />

import { describe, expect, it } from 'vitest';
import en from '../i18n/en.json';
import { recencyLegend } from './folderMapColors';
import { DIAGRAM_DIFF_MODES } from './diagramDiff';

/// Every app source file as raw text, keyed by path relative to this file.
/// Test files are excluded on purpose: they must not keep keys alive
/// (this file's own allowlist literals would otherwise self-satisfy the
/// scan). The i18n dictionaries are .json and never matched.
const sources = import.meta.glob<string>(['../**/*.svelte', '../**/*.ts', '!../**/*.test.ts'], {
  query: '?raw',
  import: 'default',
  eager: true,
});

/// Keys constructed at runtime — see the header comment for the call sites.
const dynamicKeys = new Set<string>([
  // 1. DiagramView.svelte:  $t(`diagram.colourBy.recency.${stop.key}`)
  ...recencyLegend().map((stop) => `diagram.colourBy.recency.${stop.key}`),
  // 2. DiagramDiffStep.svelte:  $t(`walkthrough.diagramDiff.mode.${…}`)
  ...DIAGRAM_DIFF_MODES.map(
    (mode) => `walkthrough.diagramDiff.mode.${mode === 'changed-only' ? 'changedOnly' : mode}`,
  ),
]);

function isReferenced(key: string, blob: string): boolean {
  return blob.includes(`'${key}'`) || blob.includes(`"${key}"`) || blob.includes(`\`${key}\``);
}

describe('i18n unused-key guard', () => {
  it('scans a plausible set of source files (glob sanity)', () => {
    const files = Object.keys(sources);
    // The app has >100 source files; a broken glob would silently mark
    // every key unused, so pin a generous lower bound instead.
    expect(files.length).toBeGreaterThan(50);
    // Pin the test-file exclusion — this guard must not scan itself.
    expect(files.some((f) => f.endsWith('.test.ts'))).toBe(false);
  });

  it('every dynamic-allowlist key exists in en.json', () => {
    // A stale allowlist entry would mask a genuinely deleted key; keep the
    // derived keys and the dictionary in lockstep (i18nParity.test.ts
    // propagates the requirement to the other locales).
    const missing = [...dynamicKeys].filter((key) => !(key in en));
    expect(missing, `dynamic allowlist keys missing from en.json:\n  - ${missing.join('\n  - ')}`).toEqual(
      [],
    );
  });

  it('every en.json key is referenced in the app sources', () => {
    const blob = Object.values(sources).join('\n');
    const unused = Object.keys(en).filter((key) => !dynamicKeys.has(key) && !isReferenced(key, blob));

    expect(
      unused,
      `${unused.length} en.json key(s) are never referenced in app/src (tests excluded).\n` +
        `Either delete them from ALL five locale files (en/de/fr/it/es — the parity test\n` +
        `enforces the rest) or, if a key is constructed dynamically at runtime, add it to\n` +
        `the documented dynamicKeys allowlist in this test:\n  - ${unused.join('\n  - ')}`,
    ).toEqual([]);
  });
});
