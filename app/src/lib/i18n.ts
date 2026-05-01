/// Tiny i18n layer for ProjectMind.
///
/// Why hand-rolled? The string surface is small (a few dozen entries) and
/// pulling in `svelte-i18n` adds ~30 KB and an async-init dance for almost
/// no win. This module is ~50 lines and exposes the same `$t('key')`
/// ergonomics directly through a Svelte derived store.
///
/// Usage:
///
///     import { t, setLang, currentLang } from '../lib/i18n';
///     // template:   {$t('nav.files')}
///     // imperative: setLang('de');
///
/// Translation files live in `src/i18n/{en,de,fr,it,es}.json`. A missing key falls
/// back to English; if the English entry is also missing, the key itself
/// is returned so dev-time misses are visible rather than silent.

import { derived, writable, type Readable } from 'svelte/store';
import en from '../i18n/en.json';
import de from '../i18n/de.json';
import fr from '../i18n/fr.json';
import it from '../i18n/it.json';
import es from '../i18n/es.json';

export type Lang = 'en' | 'de' | 'fr' | 'it' | 'es';

const STORAGE_KEY = 'projectmind.lang';

const dictionaries: Record<Lang, Record<string, string>> = { en, de, fr, it, es };

export const languages: Array<{ code: Lang; label: string }> = [
  { code: 'en', label: 'EN' },
  { code: 'de', label: 'DE' },
  { code: 'fr', label: 'FR' },
  { code: 'it', label: 'IT' },
  { code: 'es', label: 'ES' },
];

function isLang(value: string | null): value is Lang {
  return value === 'en' || value === 'de' || value === 'fr' || value === 'it' || value === 'es';
}

function detectInitialLang(): Lang {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (isLang(saved)) return saved;
  } catch {
    // localStorage unavailable
  }
  if (typeof navigator !== 'undefined') {
    const nav = navigator.language?.split('-')[0].toLowerCase() ?? '';
    if (isLang(nav)) return nav;
  }
  return 'en';
}

export const currentLang = writable<Lang>(detectInitialLang());

currentLang.subscribe((lang) => {
  try {
    localStorage.setItem(STORAGE_KEY, lang);
  } catch {
    // ignore
  }
  if (typeof document !== 'undefined') {
    document.documentElement.lang = lang;
  }
});

export function setLang(lang: Lang): void {
  currentLang.set(lang);
}

/// Reactive translator. Read with `$t('some.key')` in templates, or
/// `get(t)('some.key')` outside reactivity.
export const t: Readable<(key: string, params?: Record<string, string | number>) => string> =
  derived(currentLang, ($lang) => {
    const dict = dictionaries[$lang] ?? {};
    const fallback = dictionaries.en;
    return (key: string, params?: Record<string, string | number>) => {
      let raw = dict[key] ?? fallback[key] ?? key;
      if (params) {
        for (const [k, v] of Object.entries(params)) {
          raw = raw.replace(new RegExp(`\\{${k}\\}`, 'g'), String(v));
        }
      }
      return raw;
    };
  });

/// Static translator for non-reactive code paths (e.g. error messages built
/// during async work). Synchronously reads the current language.
export function tr(key: string, params?: Record<string, string | number>): string {
  let lang: Lang = 'en';
  currentLang.subscribe((v) => (lang = v))();
  const dict = dictionaries[lang] ?? {};
  const fallback = dictionaries.en;
  let raw = dict[key] ?? fallback[key] ?? key;
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      raw = raw.replace(new RegExp(`\\{${k}\\}`, 'g'), String(v));
    }
  }
  return raw;
}
