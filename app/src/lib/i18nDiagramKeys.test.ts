/// Pins #176: every diagram card in the Diagrams sidebar needs a
/// translated title + description in every dictionary. A missing key
/// makes `t()` fall through to the raw key ('diagram.activityHeatmap'),
/// which then renders literally in the sidebar.

import { describe, expect, it } from 'vitest';
import type { DiagramKind } from './navigation';
import en from '../i18n/en.json';
import de from '../i18n/de.json';
import fr from '../i18n/fr.json';
import itDict from '../i18n/it.json';
import es from '../i18n/es.json';

const dictionaries: Record<string, Record<string, string>> = {
  en,
  de,
  fr,
  it: itDict,
  es,
};

/// Exhaustive over `DiagramKind` — adding a new kind to the union in
/// navigation.ts fails compilation here until it is listed (and thereby
/// covered by this test).
const allKinds: Record<DiagramKind, true> = {
  'bean-graph': true,
  'bean-graph-live': true,
  'package-tree': true,
  'folder-map': true,
  'inheritance-tree': true,
  'doc-graph': true,
  'c4-container': true,
  'c4-model': true,
  'architecture-layers': true,
  'architecture-flow': true,
  'module-chord': true,
  'activity-heatmap': true,
  'timeline-river': true,
  'language-stats': true,
  'code-city': true,
};

/// Mirrors the kebab-case → camelCase key derivation DiagramIndex.svelte
/// uses in its labelFor/descriptionFor switches ('activity-heatmap' →
/// 'activityHeatmap').
function i18nKeySuffix(kind: string): string {
  return kind.replace(/-([a-z])/g, (_m, c: string) => c.toUpperCase());
}

describe('diagram card i18n coverage (#176)', () => {
  for (const [locale, dict] of Object.entries(dictionaries)) {
    for (const kind of Object.keys(allKinds)) {
      const suffix = i18nKeySuffix(kind);
      it(`${locale}: title + description for ${kind}`, () => {
        expect(dict[`diagram.${suffix}`], `diagram.${suffix} missing in ${locale}.json`).toBeTruthy();
        expect(
          dict[`diagram.description.${suffix}`],
          `diagram.description.${suffix} missing in ${locale}.json`,
        ).toBeTruthy();
      });
    }
  }
});
