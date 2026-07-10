/// Pins full i18n parity: fr/it/es silently drifted 137 keys behind
/// en.json because only the diagram-card keys were guarded
/// (i18nDiagramKeys.test.ts). The en fallback in i18n.ts (`dict[key] ??
/// en[key] ?? key`) hides such gaps at runtime — nothing crashes, the UI
/// just renders mixed-language. This test forces every locale to carry
/// EXACTLY the same key set as the en.json reference, in both directions:
/// missing keys AND surplus keys fail with a per-key listing.

import { describe, expect, it } from 'vitest';
import en from '../i18n/en.json';
import de from '../i18n/de.json';
import fr from '../i18n/fr.json';
import itDict from '../i18n/it.json';
import es from '../i18n/es.json';

const locales: Record<string, Record<string, unknown>> = {
  en,
  de,
  fr,
  it: itDict,
  es,
};

/// Collects dotted key paths recursively, so the guard keeps working even
/// if the dictionaries ever move from flat "a.b.c" keys to nested objects.
function collectKeys(value: unknown, prefix = ''): string[] {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    return [prefix];
  }
  return Object.entries(value as Record<string, unknown>).flatMap(([key, child]) =>
    collectKeys(child, prefix ? `${prefix}.${key}` : key),
  );
}

const referenceKeys = new Set(collectKeys(en));

describe('i18n key parity with en.json', () => {
  it('reference dictionary en.json is non-empty', () => {
    expect(referenceKeys.size).toBeGreaterThan(0);
  });

  for (const [locale, dict] of Object.entries(locales)) {
    it(`${locale}.json has exactly the same key set as en.json`, () => {
      const keys = new Set(collectKeys(dict));
      const missing = [...referenceKeys].filter((key) => !keys.has(key));
      const surplus = [...keys].filter((key) => !referenceKeys.has(key));

      const problems: string[] = [];
      if (missing.length > 0) {
        problems.push(
          `${missing.length} key(s) missing — translate them from en.json:\n  - ${missing.join('\n  - ')}`,
        );
      }
      if (surplus.length > 0) {
        problems.push(
          `${surplus.length} surplus key(s) not in en.json — remove them (or add them to en.json first):\n  - ${surplus.join('\n  - ')}`,
        );
      }

      expect(problems, `${locale}.json is out of sync with en.json:\n${problems.join('\n')}`).toEqual([]);
    });
  }
});
