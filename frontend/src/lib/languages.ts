// Language settings shared by Settings, the per-project table, the enhance picker and the
// meeting info bubble (spec 2026-10-30). Values: 'auto' keeps the spoken language,
// 'auto-translate' makes Whisper translate to English, a code forces that language.

export const LANGUAGES: { code: string; label: string }[] = [
  { code: 'en', label: 'English' },
  { code: 'fr', label: 'French' },
  { code: 'de', label: 'German' },
  { code: 'es', label: 'Spanish' },
  { code: 'it', label: 'Italian' },
  { code: 'nl', label: 'Dutch' },
  { code: 'pt', label: 'Portuguese' },
];

/** English name of a code, the code itself when unknown. */
export function languageName(code: string): string {
  return LANGUAGES.find((l) => l.code === code)?.label ?? code;
}

/** Label of a language setting value as the selects show it. */
export function languageSettingLabel(value: string | null | undefined): string {
  if (!value || value === 'auto') return 'Keep the language';
  if (value === 'auto-translate') return 'Translate to English';
  return `Always ${languageName(value)}`;
}

/** Options of the language selects: the two modes, then every language. */
export const LANGUAGE_OPTIONS: { value: string; label: string }[] = [
  { value: 'auto', label: 'Keep the language' },
  { value: 'auto-translate', label: 'Translate to English' },
  ...LANGUAGES.map((l) => ({ value: l.code, label: `Always ${l.label}` })),
];

/** `transcription` of metadata.json, as get_meeting_extras returns it. */
export interface TranscriptionInfo {
  live?: { provider?: string; model?: string; language?: string | null } | null;
  enhanced?: { provider?: string; model?: string; language?: string | null; status?: string } | null;
}

function providerName(provider?: string): string {
  return provider ? provider.charAt(0).toUpperCase() + provider.slice(1) : '';
}

function modelLabel(engine?: { provider?: string; model?: string } | null): string {
  if (!engine?.model) return '';
  return [providerName(engine.provider), engine.model].filter(Boolean).join(' ');
}

/** Suffix for a forced language: "to English" for translate, the language name for a code, "" otherwise. */
export function languageSuffix(language?: string | null): string {
  if (!language || language === 'auto') return '';
  if (language === 'auto-translate') return 'to English';
  return languageName(language);
}

/** Live row of the info bubble; "Unknown" for meetings recorded before the key existed. */
export function liveLabel(t?: TranscriptionInfo | null): string {
  const parts = [modelLabel(t?.live), languageSuffix(t?.live?.language)].filter(Boolean);
  return parts.length ? parts.join(' · ') : 'Unknown';
}

const ENHANCE_STATUS: Record<string, string> = { done: 'Done', queued: 'Queued', running: 'Running', failed: 'Failed' };

/** Enhanced row of the info bubble: status word, then the engine, then a forced language. */
export function enhancedLabel(status?: string, t?: TranscriptionInfo | null): string {
  const word = (status && ENHANCE_STATUS[status]) || 'Not run';
  return [word, modelLabel(t?.enhanced), languageSuffix(t?.enhanced?.language)].filter(Boolean).join(' · ');
}
