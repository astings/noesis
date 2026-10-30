import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Check, Download, Loader2 } from 'lucide-react';
import { Switch } from '@/components/ui/switch';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { SettingsGroup, SettingRow } from '@/components/SettingsGroup';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useConfig } from '@/contexts/ConfigContext';
import { MODEL_CONFIGS } from '@/lib/whisper';
import { getModelDisplayInfo } from '@/lib/parakeet';
import type { RecordingPreferences } from '@/components/RecordingSettings';

type Engine = 'whisper' | 'parakeet' | 'builtin';
type ModelState = { kind: 'missing' } | { kind: 'downloading'; progress: number } | { kind: 'ready' } | { kind: 'broken' };
interface ModelRow { engine: Engine; name: string; label: string; description: string; sizeMb: number; state: ModelState }

const DOWNLOAD_CMD: Record<Engine, string> = { whisper: 'whisper_download_model', parakeet: 'parakeet_download_model', builtin: 'builtin_ai_download_model' };
const CANCEL_CMD: Record<Engine, string> = { whisper: 'whisper_cancel_download', parakeet: 'parakeet_cancel_download', builtin: 'builtin_ai_cancel_download' };

// Whisper and Parakeet report 'Available' | 'Missing' | { Downloading } | { Error } | { Corrupted }; built-in reports { type: ... }.
function toState(status: any): ModelState {
  if (status === 'Available' || status?.type === 'available') return { kind: 'ready' };
  if (status === 'Missing' || status?.type === 'not_downloaded') return { kind: 'missing' };
  if (status?.Downloading) return { kind: 'downloading', progress: status.Downloading.progress };
  if (status?.type === 'downloading') return { kind: 'downloading', progress: status.progress };
  return { kind: 'broken' };
}

function formatSize(mb: number) {
  return mb >= 1000 ? `${(mb / 1024).toFixed(1)} GB` : `${Math.round(mb)} MB`;
}

const LANGUAGES = [
  { code: 'en', label: 'English' },
  { code: 'fr', label: 'French' },
  { code: 'de', label: 'German' },
  { code: 'es', label: 'Spanish' },
  { code: 'it', label: 'Italian' },
  { code: 'nl', label: 'Dutch' },
  { code: 'pt', label: 'Portuguese' },
];

const SUMMARY_PROVIDERS: { value: string; label: string; placeholder: string }[] = [
  { value: 'ollama', label: 'Ollama', placeholder: 'llama3.2' },
  { value: 'claude', label: 'Claude', placeholder: 'claude-haiku-4-5' },
  { value: 'openai', label: 'OpenAI', placeholder: 'gpt-4o-mini' },
  { value: 'groq', label: 'Groq', placeholder: 'llama-3.1-8b-instant' },
  { value: 'openrouter', label: 'OpenRouter', placeholder: 'openai/gpt-4o-mini' },
];
const KEYED_PROVIDERS = ['claude', 'openai', 'groq', 'openrouter'];

/** Model lists for the three engines, kept current through the download events. */
function useModelRows() {
  const [rows, setRows] = useState<ModelRow[]>([]);

  const refresh = useCallback(async () => {
    const [whisper, parakeet, builtin] = await Promise.all([
      invoke<any[]>('whisper_get_available_models').catch(() => []),
      invoke<any[]>('parakeet_get_available_models').catch(() => []),
      invoke<any[]>('builtin_ai_list_models').catch(() => []),
    ]);
    setRows([
      ...parakeet.map((m): ModelRow => {
        const info = getModelDisplayInfo(m.name);
        return { engine: 'parakeet', name: m.name, label: `Parakeet ${info?.friendlyName ?? m.name}`, description: `${m.name}. ${info?.tagline ?? m.description ?? ''}${m.name.includes('v2') ? ' English only.' : ''}`, sizeMb: m.size_mb, state: toState(m.status) };
      }),
      ...whisper.map((m): ModelRow => ({ engine: 'whisper', name: m.name, label: `Whisper ${m.name}`, description: MODEL_CONFIGS[m.name]?.description ?? m.description ?? '', sizeMb: m.size_mb, state: toState(m.status) })),
      ...builtin.map((m): ModelRow => ({ engine: 'builtin', name: m.name, label: m.display_name || m.name, description: m.description ?? '', sizeMb: m.size_mb, state: toState(m.status) })),
    ]);
  }, []);

  useEffect(() => {
    refresh();
    const setProgress = (engine: Engine, name: string, progress: number) =>
      setRows((prev) => prev.map((r) => (r.engine === engine && r.name === name ? { ...r, state: { kind: 'downloading', progress } } : r)));
    const subs = [
      listen<{ modelName: string; progress: number }>('model-download-progress', (e) => setProgress('whisper', e.payload.modelName, e.payload.progress)),
      listen<{ modelName: string; progress: number }>('parakeet-model-download-progress', (e) => setProgress('parakeet', e.payload.modelName, e.payload.progress)),
      listen<{ model: string; progress: number; status: string; error?: string }>('builtin-ai-download-progress', (e) => {
        if (e.payload.status === 'downloading') setProgress('builtin', e.payload.model, e.payload.progress);
        else refresh();
        if (e.payload.status === 'error') toast.error(`Download failed: ${e.payload.error ?? e.payload.model}`);
      }),
      listen('model-download-complete', refresh),
      listen('parakeet-model-download-complete', refresh),
      listen<{ modelName: string; error: string }>('model-download-error', (e) => { refresh(); toast.error(`Download failed: ${e.payload.error}`); }),
      listen<{ modelName: string; error: string }>('parakeet-model-download-error', (e) => { refresh(); toast.error(`Download failed: ${e.payload.error}`); }),
    ];
    return () => { subs.forEach((p) => p.then((fn) => fn())); };
  }, [refresh]);

  return { rows, refresh };
}

/** One button: download, progress (click cancels), or downloaded. */
function DownloadButton({ row, onChanged }: { row: ModelRow; onChanged: () => void }) {
  const { state, engine, name } = row;
  if (state.kind === 'ready') {
    return <span className="inline-flex h-7 items-center gap-1 px-2 text-xs text-muted-foreground"><Check className="h-3.5 w-3.5" />Downloaded</span>;
  }
  if (state.kind === 'downloading') {
    return (
      <Button size="sm" variant="ghost" className="h-7 w-[104px] text-xs" title="Cancel download"
        onClick={() => invoke(CANCEL_CMD[engine], { modelName: name }).catch((e) => toast.error(String(e))).finally(onChanged)}>
        <Loader2 className="animate-spin" />{Math.round(state.progress)}%
      </Button>
    );
  }
  return (
    <Button size="sm" variant="outline" className="h-7 w-[104px] text-xs"
      onClick={() => {
        // fire and forget: progress arrives through the download events
        invoke(DOWNLOAD_CMD[engine], { modelName: name }).catch((e) => { toast.error(String(e)); onChanged(); });
      }}>
      <Download />{state.kind === 'broken' ? 'Retry' : formatSize(row.sizeMb)}
    </Button>
  );
}

function Tick({ checked, disabled, label, onChange }: { checked: boolean; disabled: boolean; label: string; onChange: () => void }) {
  return (
    <input type="checkbox" aria-label={label} title={disabled ? 'Download the model first' : label}
      className="h-4 w-4 cursor-pointer accent-foreground disabled:cursor-not-allowed disabled:opacity-30"
      checked={checked} disabled={disabled} onChange={onChange} />
  );
}

function ModelTable({ rows, columns, onChanged }: { rows: ModelRow[]; columns: { label: string; checked: (r: ModelRow) => boolean; pick: (r: ModelRow) => void }[]; onChanged: () => void }) {
  return (
    <table className="w-full text-[13px]">
      <thead>
        <tr className="text-xs text-muted-foreground">
          <th className="px-3.5 py-2 text-left font-normal">Model</th>
          <th className="w-[112px]" />
          {columns.map((c) => <th key={c.label} className="w-16 px-2 py-2 text-center font-normal">{c.label}</th>)}
        </tr>
      </thead>
      <tbody className="divide-y divide-border">
        {rows.map((r) => (
          <tr key={`${r.engine}:${r.name}`}>
            <td className="px-3.5 py-1.5">
              <Tooltip>
                <TooltipTrigger asChild><span className="cursor-default">{r.label}</span></TooltipTrigger>
                {r.description && <TooltipContent side="right" className="max-w-xs">{r.description}</TooltipContent>}
              </Tooltip>
            </td>
            <td className="py-1.5"><DownloadButton row={r} onChanged={onChanged} /></td>
            {columns.map((c) => (
              <td key={c.label} className="px-2 py-1.5 text-center">
                <Tick label={`${c.label}: ${r.label}`} checked={c.checked(r)} disabled={r.state.kind !== 'ready'} onChange={() => c.pick(r)} />
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function ModelsSettings() {
  const { rows, refresh } = useModelRows();
  const { transcriptModelConfig, setTranscriptModelConfig, selectedLanguage, setSelectedLanguage } = useConfig();
  const [prefs, setPrefs] = useState<RecordingPreferences | null>(null);
  const [saving, setSaving] = useState(false);

  // Summary model (the same settings row the summaries use)
  const [summary, setSummary] = useState({ provider: 'builtin-ai', model: '', whisperModel: 'large-v3', endpoint: '', hasKey: false });
  const [externalDraft, setExternalDraft] = useState<{ provider: string; model: string; endpoint: string; apiKey: string } | null>(null);
  const [defaultPrompt, setDefaultPrompt] = useState('');
  const [promptDraft, setPromptDraft] = useState<string | null>(null);
  const [taggingAll, setTaggingAll] = useState(false);
  // Ask during meetings (live ask spec 3.4): provider and model preferences; keys are shared per provider
  const [askDraft, setAskDraft] = useState<{ provider: string; model: string; apiKey: string } | null>(null);
  const [askHasKey, setAskHasKey] = useState(false);
  const askProvider = askDraft?.provider ?? prefs?.ask_provider ?? '';
  useEffect(() => {
    if (!KEYED_PROVIDERS.includes(askProvider)) { setAskHasKey(false); return; }
    invoke<string>('api_get_api_key', { provider: askProvider }).then((k) => setAskHasKey(!!k)).catch(() => setAskHasKey(false));
  }, [askProvider]);

  useEffect(() => {
    invoke<RecordingPreferences>('get_recording_preferences').then(setPrefs).catch((e) => toast.error(String(e)));
    invoke<string>('get_default_title_tags_prompt').then(setDefaultPrompt).catch(() => {});
    invoke<{ provider: string; model: string; whisperModel: string; apiKey: string | null; ollamaEndpoint: string | null } | null>('api_get_model_config')
      .then((c) => c && setSummary({ provider: c.provider, model: c.model, whisperModel: c.whisperModel || 'large-v3', endpoint: c.ollamaEndpoint ?? '', hasKey: !!c.apiKey }))
      .catch((e) => console.error('Failed to load summary model', e));
  }, []);

  const applyPrefs = async (next: RecordingPreferences) => {
    setPrefs(next);
    setSaving(true);
    try {
      await invoke('set_recording_preferences', { preferences: next });
    } catch (e) {
      toast.error('Failed to save', { description: String(e) });
    } finally {
      setSaving(false);
    }
  };

  const pickLive = async (r: ModelRow) => {
    const provider = r.engine === 'parakeet' ? 'parakeet' : 'localWhisper';
    try {
      await invoke('api_save_transcript_config', { provider, model: r.name, apiKey: null });
      setTranscriptModelConfig({ provider, model: r.name, apiKey: null });
    } catch (e) {
      toast.error(String(e));
    }
  };

  const saveSummary = async (provider: string, model: string, endpoint: string | null, apiKey: string | null) => {
    try {
      await invoke('api_save_model_config', { provider, model, whisperModel: summary.whisperModel, apiKey, ollamaEndpoint: endpoint });
      setSummary((s) => ({ ...s, provider, model, endpoint: endpoint ?? '', hasKey: apiKey ? true : s.hasKey && s.provider === provider }));
      setExternalDraft(null);
      toast.success('Summary model saved');
    } catch (e) {
      toast.error(String(e));
    }
  };

  const commitPrompt = async () => {
    if (!prefs || promptDraft === null) return;
    const text = promptDraft.trim() === defaultPrompt.trim() ? '' : promptDraft;
    setPromptDraft(null);
    if (text !== prefs.title_tags_prompt) await applyPrefs({ ...prefs, title_tags_prompt: text });
  };

  const tagUntagged = async () => {
    setTaggingAll(true);
    try {
      toast.success(await invoke<string>('tag_untagged_meetings'));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setTaggingAll(false);
    }
  };

  const saveAsk = async () => {
    if (!prefs || !askDraft) return;
    const key = askDraft.apiKey.trim();
    try {
      if (key) await invoke('api_save_api_key', { provider: askDraft.provider, apiKey: key });
      await applyPrefs({ ...prefs, ask_provider: askDraft.provider, ask_model: askDraft.model.trim() });
      if (key) setAskHasKey(true);
      setAskDraft(null);
      toast.success('Ask model saved');
    } catch (e) {
      toast.error(String(e));
    }
  };

  if (!prefs) return <div className="text-sm text-muted-foreground">Loading models...</div>;

  const transcriptionRows = rows.filter((r) => r.engine !== 'builtin');
  const builtinRows = rows.filter((r) => r.engine === 'builtin');
  const isExternal = summary.provider !== 'builtin-ai';
  const external = externalDraft ?? (isExternal
    ? { provider: summary.provider, model: summary.model, endpoint: summary.endpoint, apiKey: '' }
    : { provider: 'ollama', model: '', endpoint: '', apiKey: '' });
  const externalKnown = SUMMARY_PROVIDERS.find((p) => p.value === external.provider);
  const languages = prefs.transcription_languages ?? [];
  const ask = askDraft ?? { provider: prefs.ask_provider, model: prefs.ask_model, apiKey: '' };
  const askKnown = SUMMARY_PROVIDERS.find((p) => p.value === ask.provider);
  const readyBuiltin = builtinRows.filter((r) => r.state.kind === 'ready');

  return (
    <div className="space-y-6">
      <SettingsGroup title="Audio transcription">
        <SettingRow
          label="Enhance transcript after recording"
          hint="Re-transcribe the full recording with the Enhance model as soon as it stops"
          control={<Switch aria-label="Enhance transcript after recording" checked={prefs.auto_enhance} onCheckedChange={(on) => applyPrefs({ ...prefs, auto_enhance: on })} disabled={saving} />}
        />
        <SettingRow
          label="Languages"
          hint="Whisper only picks among these. Parakeet detects the language on its own."
          control={
            <Select value={selectedLanguage} onValueChange={setSelectedLanguage}>
              <SelectTrigger aria-label="Live Whisper language" className="h-8 w-48 text-[13px]"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="auto">Live: keep the language</SelectItem>
                <SelectItem value="auto-translate">Live: translate to English</SelectItem>
                {selectedLanguage !== 'auto' && selectedLanguage !== 'auto-translate' && <SelectItem value={selectedLanguage}>Live: always {selectedLanguage}</SelectItem>}
              </SelectContent>
            </Select>
          }
        >
          <div className="flex flex-wrap gap-1.5">
            {LANGUAGES.map((l) => {
              const on = languages.includes(l.code);
              return (
                <button key={l.code} type="button" aria-pressed={on} disabled={saving}
                  className={`rounded border px-2 py-0.5 text-xs ${on ? 'border-foreground bg-foreground text-background' : 'border-border text-muted-foreground hover:text-foreground'}`}
                  onClick={() => applyPrefs({ ...prefs, transcription_languages: on ? languages.filter((c) => c !== l.code) : [...languages, l.code] })}>
                  {l.label}
                </button>
              );
            })}
          </div>
        </SettingRow>
        <ModelTable
          rows={transcriptionRows}
          onChanged={refresh}
          columns={[
            {
              label: 'Live',
              checked: (r) => r.name === transcriptModelConfig.model && (r.engine === 'parakeet') === (transcriptModelConfig.provider === 'parakeet'),
              pick: pickLive,
            },
            {
              label: 'Enhance',
              checked: (r) => r.engine === prefs.enhance_provider && r.name === prefs.enhance_model,
              pick: (r) => applyPrefs({ ...prefs, enhance_provider: r.engine as 'whisper' | 'parakeet', enhance_model: r.name }),
            },
          ]}
        />
      </SettingsGroup>

      <SettingsGroup title="Summarization">
        <SettingRow
          label="Name and tag meetings after enhance"
          hint="Off: run it from the meeting page or the right-click menu"
          control={<Switch aria-label="Name and tag meetings after enhance" checked={prefs.auto_title_tags} onCheckedChange={(on) => applyPrefs({ ...prefs, auto_title_tags: on })} disabled={saving} />}
        />
        <ModelTable
          rows={builtinRows}
          onChanged={refresh}
          columns={[{
            label: 'Use',
            checked: (r) => !isExternal && r.name === summary.model,
            pick: (r) => saveSummary('builtin-ai', r.name, null, null),
          }]}
        />
        <SettingRow
          label="Other provider"
          hint="Ollama stays on this Mac; the others send the transcript to their API"
          control={<Tick label="Use: other provider" checked={isExternal} disabled={false} onChange={() => setExternalDraft(external)} />}
        >
          {(isExternal || externalDraft) && (
            <div className="flex flex-wrap items-center gap-2">
              <Select value={external.provider} onValueChange={(v) => setExternalDraft({ ...external, provider: v, apiKey: '' })}>
                <SelectTrigger aria-label="Provider" className="h-8 w-32 text-[13px]"><SelectValue /></SelectTrigger>
                <SelectContent>
                  {SUMMARY_PROVIDERS.map((p) => <SelectItem key={p.value} value={p.value}>{p.label}</SelectItem>)}
                  {!externalKnown && <SelectItem value={external.provider} disabled>Custom (configured elsewhere)</SelectItem>}
                </SelectContent>
              </Select>
              <Input aria-label="Model" className="h-8 w-48 text-[13px]" value={external.model} placeholder={externalKnown?.placeholder} onChange={(e) => setExternalDraft({ ...external, model: e.target.value })} />
              {external.provider === 'ollama' && (
                <Input aria-label="Ollama endpoint" className="h-8 w-48 text-[13px]" value={external.endpoint} placeholder="http://localhost:11434" onChange={(e) => setExternalDraft({ ...external, endpoint: e.target.value })} />
              )}
              {KEYED_PROVIDERS.includes(external.provider) && (
                <Input aria-label="API key" type="password" autoComplete="off" className="h-8 w-40 text-[13px]" value={external.apiKey}
                  placeholder={summary.hasKey && summary.provider === external.provider ? 'Saved' : 'Paste a key'}
                  onChange={(e) => setExternalDraft({ ...external, apiKey: e.target.value })} />
              )}
              <Button size="sm" variant="outline" disabled={!external.model.trim()}
                onClick={() => saveSummary(external.provider, external.model.trim(), external.provider === 'ollama' ? external.endpoint.trim() || null : null, external.apiKey.trim() || null)}>
                Save
              </Button>
            </div>
          )}
        </SettingRow>
        <SettingRow
          label="Prompt for titles and tags"
          hint="System prompt sent with the transcript. The reply must stay the JSON shape it asks for."
          control={
            <Button size="sm" variant="ghost" disabled={saving || !prefs.title_tags_prompt} onClick={() => { setPromptDraft(null); applyPrefs({ ...prefs, title_tags_prompt: '' }); }}>
              Reset
            </Button>
          }
        >
          <Textarea aria-label="Prompt for titles and tags" rows={5} className="font-mono text-xs"
            value={promptDraft ?? (prefs.title_tags_prompt || defaultPrompt)}
            onChange={(e) => setPromptDraft(e.target.value)} onBlur={commitPrompt} disabled={saving} />
        </SettingRow>
        <SettingRow
          label="Tag meetings without tags"
          control={
            <Button size="sm" variant="outline" onClick={tagUntagged} disabled={taggingAll}>
              {taggingAll && <Loader2 className="animate-spin" />}
              Run now
            </Button>
          }
        />
      </SettingsGroup>
      <SettingsGroup title="Ask during meetings">
        <SettingRow
          label="Model"
          hint="Answers questions about the transcript from the Ask box. Built-in and Ollama stay on this Mac; the others send the transcript to their API."
        >
          <div className="flex flex-wrap items-center gap-2">
            <Select value={ask.provider || 'summary'} onValueChange={(v) => setAskDraft({ provider: v === 'summary' ? '' : v, model: '', apiKey: '' })}>
              <SelectTrigger aria-label="Ask provider" className="h-8 w-52 text-[13px]"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="summary">Same as summary model</SelectItem>
                <SelectItem value="builtin-ai">Built-in</SelectItem>
                {SUMMARY_PROVIDERS.map((p) => <SelectItem key={p.value} value={p.value}>{p.label}</SelectItem>)}
              </SelectContent>
            </Select>
            {ask.provider === 'builtin-ai' && (
              <Select value={ask.model} onValueChange={(v) => setAskDraft({ ...ask, model: v })}>
                <SelectTrigger aria-label="Ask built-in model" className="h-8 w-52 text-[13px]"><SelectValue placeholder="Pick a downloaded model" /></SelectTrigger>
                <SelectContent>
                  {readyBuiltin.map((r) => <SelectItem key={r.name} value={r.name}>{r.label}</SelectItem>)}
                </SelectContent>
              </Select>
            )}
            {ask.provider && ask.provider !== 'builtin-ai' && (
              <Input aria-label="Ask model" className="h-8 w-48 text-[13px]" value={ask.model} placeholder={askKnown?.placeholder}
                onChange={(e) => setAskDraft({ ...ask, model: e.target.value })} />
            )}
            {KEYED_PROVIDERS.includes(ask.provider) && (
              <Input aria-label="Ask API key" type="password" autoComplete="off" className="h-8 w-40 text-[13px]" value={ask.apiKey}
                placeholder={askHasKey ? 'Saved' : 'Paste a key'} onChange={(e) => setAskDraft({ ...ask, apiKey: e.target.value })} />
            )}
            {askDraft && (
              <Button size="sm" variant="outline" disabled={saving || (!!ask.provider && !ask.model.trim())} onClick={saveAsk}>
                Save
              </Button>
            )}
          </div>
        </SettingRow>
      </SettingsGroup>
    </div>
  );
}
