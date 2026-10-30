import { invoke } from '@tauri-apps/api/core';

/** A downloaded transcription model, as the enhance pickers list them. */
export interface TranscriptionModel { provider: 'whisper' | 'parakeet'; model: string; label: string }

// Whisper and Parakeet report 'Available' | 'Missing' | { Downloading } | { Error } | { Corrupted }.
const ready = (status: unknown) => status === 'Available';

/** Downloaded Whisper and Parakeet models, Parakeet first like the Settings table. */
export async function listReadyTranscriptionModels(): Promise<TranscriptionModel[]> {
  const [whisper, parakeet] = await Promise.all([
    invoke<{ name: string; status: unknown }[]>('whisper_get_available_models').catch(() => []),
    invoke<{ name: string; status: unknown }[]>('parakeet_get_available_models').catch(() => []),
  ]);
  return [
    ...parakeet.filter((m) => ready(m.status)).map((m): TranscriptionModel => ({ provider: 'parakeet', model: m.name, label: `Parakeet ${m.name}` })),
    ...whisper.filter((m) => ready(m.status)).map((m): TranscriptionModel => ({ provider: 'whisper', model: m.name, label: `Whisper ${m.name}` })),
  ];
}
