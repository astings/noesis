import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { INBOX } from '@/lib/projects';
import { LANGUAGE_OPTIONS, languageSettingLabel } from '@/lib/languages';
import type { TranscriptionModel } from '@/lib/models';

/** `<root>/<project>/project.json`; null means the global setting (spec 2026-10-30 section 3.1). */
export interface ProjectSettings { language: string | null; enhance_provider: string | null; enhance_model: string | null }

const EMPTY: ProjectSettings = { language: null, enhance_provider: null, enhance_model: null };
const DEFAULT = '__default__';
const key = (provider: string, model: string) => `${provider}:${model}`;
const providerName = (p: string) => p.charAt(0).toUpperCase() + p.slice(1);

/** One row per project; a change writes project.json at once. `models` are the downloaded transcription models. */
export function ProjectOverrides({ models, globalLanguage, globalProvider, globalModel }: {
  models: TranscriptionModel[];
  globalLanguage: string;
  globalProvider: string;
  globalModel: string;
}) {
  const [projects, setProjects] = useState<string[]>([]);
  const [settings, setSettings] = useState<Record<string, ProjectSettings>>({});

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const list = await invoke<string[]>('list_projects_command').catch(() => [INBOX]);
      const entries = await Promise.all(list.map(async (p) => [p, await invoke<ProjectSettings>('get_project_settings', { project: p }).catch(() => EMPTY)] as const));
      if (cancelled) return;
      setProjects(list);
      setSettings(Object.fromEntries(entries));
    })();
    return () => { cancelled = true; };
  }, []);

  const save = async (project: string, next: ProjectSettings) => {
    const prev = settings[project] ?? EMPTY;
    setSettings((s) => ({ ...s, [project]: next }));
    try {
      await invoke('set_project_settings', { project, settings: next });
    } catch (e) {
      setSettings((s) => ({ ...s, [project]: prev }));
      toast.error('Could not save the project settings', { description: String(e) });
    }
  };

  if (!projects.length) return null;
  return (
    <table className="w-full text-[13px]">
      <thead>
        <tr className="text-xs text-muted-foreground">
          <th className="py-2 text-left font-normal">Project</th>
          <th className="px-2 py-2 text-left font-normal">Language</th>
          <th className="px-2 py-2 text-left font-normal">Enhance model</th>
        </tr>
      </thead>
      <tbody className="divide-y divide-border">
        {projects.map((p) => {
          const s = settings[p] ?? EMPTY;
          const modelValue = s.enhance_provider && s.enhance_model ? key(s.enhance_provider, s.enhance_model) : DEFAULT;
          const known = modelValue === DEFAULT || models.some((m) => key(m.provider, m.model) === modelValue);
          return (
            <tr key={p}>
              <td className="py-1.5">{p === INBOX ? 'Inbox' : p}</td>
              <td className="px-2 py-1.5">
                <Select value={s.language ?? DEFAULT} onValueChange={(v) => save(p, { ...s, language: v === DEFAULT ? null : v })}>
                  <SelectTrigger aria-label={`Language for ${p}`} className="h-8 w-52 text-[13px]"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value={DEFAULT}>Default ({languageSettingLabel(globalLanguage)})</SelectItem>
                    {LANGUAGE_OPTIONS.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}
                  </SelectContent>
                </Select>
              </td>
              <td className="px-2 py-1.5">
                <Select
                  value={modelValue}
                  onValueChange={(v) => {
                    const m = models.find((x) => key(x.provider, x.model) === v);
                    save(p, { ...s, enhance_provider: m?.provider ?? null, enhance_model: m?.model ?? null });
                  }}
                >
                  <SelectTrigger aria-label={`Enhance model for ${p}`} className="h-8 w-52 text-[13px]"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value={DEFAULT}>Default ({providerName(globalProvider)} {globalModel})</SelectItem>
                    {models.map((m) => <SelectItem key={key(m.provider, m.model)} value={key(m.provider, m.model)}>{m.label}</SelectItem>)}
                    {!known && <SelectItem value={modelValue} disabled>{modelValue.replace(':', ' ')} (not downloaded)</SelectItem>}
                  </SelectContent>
                </Select>
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
