"use client";

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogTitle } from '@/components/ui/dialog';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Button } from '@/components/ui/button';
import { LANGUAGE_OPTIONS, languageSettingLabel } from '@/lib/languages';
import { listReadyTranscriptionModels, type TranscriptionModel } from '@/lib/models';
import { startEnhance } from '@/lib/autoEnhance';

const DEFAULT = '__default__';
const key = (provider: string, model: string) => `${provider}:${model}`;
interface Resolved { language: string | null; provider: string; model: string }

/**
 * spec 2026-10-30 section 3.5: one-off language and model for this enhance run. A select left
 * on Default passes null, so Rust resolves it from project.json and Settings. Nothing persists.
 */
export function EnhanceWithDialog({ open, onOpenChange, meetingId, folderPath }: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  meetingId: string;
  folderPath: string;
}) {
  const [resolved, setResolved] = useState<Resolved | null>(null);
  const [models, setModels] = useState<TranscriptionModel[]>([]);
  const [language, setLanguage] = useState(DEFAULT);
  const [model, setModel] = useState(DEFAULT);

  useEffect(() => {
    if (!open) return;
    setLanguage(DEFAULT);
    setModel(DEFAULT);
    invoke<Resolved>('resolve_enhance_settings', { meetingFolderPath: folderPath }).then(setResolved).catch(() => setResolved(null));
    listReadyTranscriptionModels().then(setModels);
  }, [open, folderPath]);

  const run = async () => {
    const picked = models.find((m) => key(m.provider, m.model) === model);
    onOpenChange(false);
    await startEnhance(meetingId, folderPath, {
      language: language === DEFAULT ? null : language,
      provider: picked?.provider ?? null,
      model: picked?.model ?? null,
    });
  };

  const hint = resolved
    ? `Default: ${languageSettingLabel(resolved.language)} · ${resolved.provider} ${resolved.model}`
    : 'Default: the project settings, then Settings';

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[420px]">
        <DialogTitle>Enhance with...</DialogTitle>
        <DialogDescription>{hint}</DialogDescription>
        <div className="grid gap-3">
          <Select value={language} onValueChange={setLanguage}>
            <SelectTrigger aria-label="Language"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value={DEFAULT}>Default language</SelectItem>
              {LANGUAGE_OPTIONS.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}
            </SelectContent>
          </Select>
          <Select value={model} onValueChange={setModel}>
            <SelectTrigger aria-label="Model"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value={DEFAULT}>Default model</SelectItem>
              {models.map((m) => <SelectItem key={key(m.provider, m.model)} value={key(m.provider, m.model)}>{m.label}</SelectItem>)}
            </SelectContent>
          </Select>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button onClick={run}>Run</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
