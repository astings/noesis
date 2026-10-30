import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { LANGUAGES } from '@/lib/languages';
import type { RecordingPreferences } from '@/components/RecordingSettings';

export function YouStep() {
  const { goNext, savePreferences } = useOnboarding();
  const [folder, setFolder] = useState('');
  const [name, setName] = useState('');
  const [languages, setLanguages] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    invoke<RecordingPreferences>('get_recording_preferences')
      .then((p) => {
        setFolder(p.save_folder);
        setName(p.owner_name);
        setLanguages(p.transcription_languages.length ? p.transcription_languages : ['en']);
      })
      .catch((e) => setError(String(e)));
  }, []);

  const changeFolder = async () => {
    const picked = await invoke<string | null>('select_directory').catch(() => null);
    if (picked) setFolder(picked);
  };

  const toggleLanguage = (code: string) =>
    setLanguages((prev) =>
      prev.includes(code) ? (prev.length > 1 ? prev.filter((c) => c !== code) : prev) : [...prev, code]
    );

  const handleContinue = async () => {
    setSaving(true);
    setError('');
    try {
      await savePreferences({
        save_folder: folder,
        owner_name: name.trim() || 'Me',
        transcription_languages: languages,
      });
      goNext();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <OnboardingContainer
      title="You and your files"
      description="Where meetings are saved and how they are labeled."
      step={2}
      showNavigation
      canGoNext={false}
    >
      <div className="max-w-md mx-auto space-y-6">
        <div className="space-y-2">
          <p className="text-sm font-medium text-foreground">Recordings folder</p>
          <div className="flex items-center gap-2">
            <p className="flex-1 font-mono text-xs break-all text-muted-foreground">{folder || 'Default folder'}</p>
            <Button variant="outline" size="sm" onClick={changeFolder} disabled={saving}>Change…</Button>
          </div>
          <p className="text-xs text-muted-foreground">Each project gets a subfolder with one folder per meeting.</p>
        </div>

        <div className="space-y-2">
          <p className="text-sm font-medium text-foreground">Your name</p>
          <Input value={name} onChange={(e) => setName(e.target.value)} aria-label="Your name" disabled={saving} />
          <p className="text-xs text-muted-foreground">Used for your side of the transcript.</p>
        </div>

        <div className="space-y-2">
          <p className="text-sm font-medium text-foreground">Meeting languages</p>
          <div className="flex flex-wrap gap-1.5">
            {LANGUAGES.map((l) => {
              const on = languages.includes(l.code);
              return (
                <button key={l.code} type="button" aria-pressed={on} disabled={saving}
                  className={`rounded border px-2 py-0.5 text-xs ${on ? 'border-foreground bg-foreground text-background' : 'border-border text-muted-foreground hover:text-foreground'}`}
                  onClick={() => toggleLanguage(l.code)}>
                  {l.label}
                </button>
              );
            })}
          </div>
        </div>

        <div className="space-y-2 pt-2">
          <Button onClick={handleContinue} disabled={saving || !folder} className="w-full h-11">Continue</Button>
          {error && <p className="text-xs text-center text-destructive">{error}</p>}
        </div>
      </div>
    </OnboardingContainer>
  );
}
