// Run with `pnpm test` (Node's built-in runner, types stripped natively).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { enhancedLabel, languageSettingLabel, languageSuffix, liveLabel } from './languages.ts';

test('live row names the engine and says Unknown for old meetings', () => {
  assert.equal(liveLabel({ live: { provider: 'parakeet', model: 'parakeet-tdt-0.6b-v3-int8' } }), 'Parakeet parakeet-tdt-0.6b-v3-int8');
  assert.equal(liveLabel({ live: { provider: 'whisper', model: 'small', language: 'auto-translate' } }), 'Whisper small · to English');
  assert.equal(liveLabel({ live: { provider: 'whisper', model: 'small', language: 'auto' } }), 'Whisper small');
  assert.equal(liveLabel(null), 'Unknown');
  assert.equal(liveLabel({}), 'Unknown');
});

test('enhanced row joins status, model and forced language', () => {
  const done = { enhanced: { provider: 'whisper', model: 'large-v3', language: 'auto-translate', status: 'done' } };
  assert.equal(enhancedLabel('done', done), 'Done · Whisper large-v3 · to English');
  assert.equal(enhancedLabel('failed', { enhanced: { provider: 'whisper', model: 'large-v3', language: 'es' } }), 'Failed · Whisper large-v3 · Spanish');
  assert.equal(enhancedLabel('running', { enhanced: { provider: 'parakeet', model: 'p', language: 'auto' } }), 'Running · Parakeet p');
  assert.equal(enhancedLabel(undefined, null), 'Not run');
  assert.equal(enhancedLabel('done', null), 'Done');
});

test('setting labels and suffixes', () => {
  assert.equal(languageSettingLabel(null), 'Keep the language');
  assert.equal(languageSettingLabel('auto'), 'Keep the language');
  assert.equal(languageSettingLabel('auto-translate'), 'Translate to English');
  assert.equal(languageSettingLabel('es'), 'Always Spanish');
  assert.equal(languageSuffix('xx'), 'xx', 'unknown code shown as is');
  assert.equal(languageSuffix(null), '');
});
