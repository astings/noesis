"use client";

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogTitle } from '@/components/ui/dialog';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Button } from '@/components/ui/button';
import { useConfig } from '@/contexts/ConfigContext';
import { INBOX } from '@/lib/projects';
import { LANGUAGE_OPTIONS, languageSettingLabel, providerName } from '@/lib/languages';
import { listReadyTranscriptionModels, type TranscriptionModel } from '@/lib/models';

/** `<root>/<project>/project.json`; null means the global setting (spec 2026-10-30 section 3.1). */
export interface ProjectSettings { language: string | null; enhance_provider: string | null; enhance_model: string | null }

export const EMPTY_PROJECT_SETTINGS: ProjectSettings = { language: null, enhance_provider: null, enhance_model: null };
const DEFAULT = '__default__';
const key = (provider: string, model: string) => `${provider}:${model}`;

/** One project's language and enhance model; Save writes project.json. */
export function ProjectSettingsDialog({ project, onOpenChange, onSaved }: {
  /** null keeps the dialog closed */
  project: string | null;
  onOpenChange: (open: boolean) => void;
  onSaved?: (project: string, settings: ProjectSettings) => void;
}) {
  const { selectedLanguage } = useConfig();
  const [models, setModels] = useState<TranscriptionModel[]>([]);
  const [globalEnhance, setGlobalEnhance] = useState<{ enhance_provider: string; enhance_model: string } | null>(null);
  const [draft, setDraft] = useState<ProjectSettings>(EMPTY_PROJECT_SETTINGS);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!project) return;
    setDraft(EMPTY_PROJECT_SETTINGS);
    invoke<ProjectSettings>('get_project_settings', { project }).then(setDraft).catch(() => {});
    invoke<{ enhance_provider: string; enhance_model: string }>('get_recording_preferences').then(setGlobalEnhance).catch(() => setGlobalEnhance(null));
    listReadyTranscriptionModels().then(setModels);
  }, [project]);

  const modelValue = draft.enhance_provider && draft.enhance_model ? key(draft.enhance_provider, draft.enhance_model) : DEFAULT;
  const known = modelValue === DEFAULT || models.some((m) => key(m.provider, m.model) === modelValue);

  const save = async () => {
    if (!project) return;
    setSaving(true);
    try {
      await invoke('set_project_settings', { project, settings: draft });
      onSaved?.(project, draft);
      onOpenChange(false);
    } catch (e) {
      toast.error('Could not save the project settings', { description: String(e) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={project !== null} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[420px]">
        <DialogTitle>{project === INBOX ? 'Inbox' : project} settings</DialogTitle>
        <DialogDescription>Language applies live and to the enhance run. Live translation needs Whisper as the Live model.</DialogDescription>
        <div className="grid gap-3">
          <label className="grid gap-1 text-xs text-muted-foreground">
            Language
            <Select value={draft.language ?? DEFAULT} onValueChange={(v) => setDraft({ ...draft, language: v === DEFAULT ? null : v })}>
              <SelectTrigger aria-label="Language"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value={DEFAULT}>Default ({languageSettingLabel(selectedLanguage)})</SelectItem>
                {LANGUAGE_OPTIONS.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}
              </SelectContent>
            </Select>
          </label>
          <label className="grid gap-1 text-xs text-muted-foreground">
            Enhance model
            <Select
              value={modelValue}
              onValueChange={(v) => {
                const m = models.find((x) => key(x.provider, x.model) === v);
                setDraft({ ...draft, enhance_provider: m?.provider ?? null, enhance_model: m?.model ?? null });
              }}
            >
              <SelectTrigger aria-label="Enhance model"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value={DEFAULT}>
                  Default{globalEnhance ? ` (${providerName(globalEnhance.enhance_provider)} ${globalEnhance.enhance_model})` : ''}
                </SelectItem>
                {models.map((m) => <SelectItem key={key(m.provider, m.model)} value={key(m.provider, m.model)}>{m.label}</SelectItem>)}
                {!known && <SelectItem value={modelValue} disabled>{modelValue.replace(':', ' ')} (not downloaded)</SelectItem>}
              </SelectContent>
            </Select>
          </label>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button onClick={save} disabled={saving}>Save</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
