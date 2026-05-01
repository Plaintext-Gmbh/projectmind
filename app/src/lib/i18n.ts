import { derived, writable } from 'svelte/store';
import en from '../i18n/en.json';
import de from '../i18n/de.json';
import fr from '../i18n/fr.json';
import it from '../i18n/it.json';
import es from '../i18n/es.json';

export type Language = 'en' | 'de' | 'fr' | 'it' | 'es';

type Dictionary = Record<string, string>;

const dictionaries: Record<Language, Dictionary> = { en, de, fr, it, es };
const STORAGE_KEY = 'projectmind.lang';

export const languages: Array<{ code: Language; label: string }> = [
  { code: 'en', label: 'EN' },
  { code: 'de', label: 'DE' },
  { code: 'fr', label: 'FR' },
  { code: 'it', label: 'IT' },
  { code: 'es', label: 'ES' },
];

function isLanguage(value: string | null): value is Language {
  return value === 'en' || value === 'de' || value === 'fr' || value === 'it' || value === 'es';
}

function browserLanguage(): Language {
  if (typeof navigator === 'undefined') return 'en';
  const base = navigator.language.split('-')[0].toLowerCase();
  return isLanguage(base) ? base : 'en';
}

function readLanguage(): Language {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (isLanguage(stored)) return stored;
  } catch {
    // localStorage unavailable
  }
  return browserLanguage();
}

export const language = writable<Language>(readLanguage());

language.subscribe((value) => {
  if (typeof document !== 'undefined') {
    document.documentElement.lang = value;
  }
  try {
    localStorage.setItem(STORAGE_KEY, value);
  } catch {
    // ignore
  }
});

export function setLanguage(value: Language) {
  language.set(value);
}

function interpolate(template: string, vars?: Record<string, string | number>): string {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (match, key) =>
    Object.prototype.hasOwnProperty.call(vars, key) ? String(vars[key]) : match,
  );
}

export const t = derived(language, ($language) => {
  const dict = dictionaries[$language];
  return (key: string, vars?: Record<string, string | number>): string =>
    interpolate(dict[key] ?? dictionaries.en[key] ?? key, vars);
});
