import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import type { RecordingPreferences } from '@/components/RecordingSettings';

type CallKey = 'auto_start_on_call' | 'auto_stop_on_call_end' | 'show_call_overlay' | 'show_tray_icon' | 'calendar_meeting_name';

const ROWS: { key: CallKey; label: string; hint?: string }[] = [
  { key: 'auto_start_on_call', label: 'Start recording when a call begins', hint: 'Get consent from everyone on the call. Recording rules differ by country.' },
  { key: 'auto_stop_on_call_end', label: 'Stop recording when the call ends' },
  { key: 'show_call_overlay', label: 'Floating call overlay', hint: 'Timer, pause, stop and project during a recording.' },
  { key: 'show_tray_icon', label: 'Menu bar icon' },
  { key: 'calendar_meeting_name', label: 'Name meetings from your calendar', hint: 'Asks for Calendar access.' },
];

export function CallsStep() {
  const { goNext, savePreferences } = useOnboarding();
  const [values, setValues] = useState<Record<CallKey, boolean>>({
    auto_start_on_call: false,
    auto_stop_on_call_end: true,
    show_call_overlay: true,
    show_tray_icon: true,
    calendar_meeting_name: false,
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    invoke<RecordingPreferences>('get_recording_preferences')
      .then((p) => setValues({
        auto_start_on_call: p.auto_start_on_call,
        auto_stop_on_call_end: p.auto_stop_on_call_end,
        show_call_overlay: p.show_call_overlay,
        show_tray_icon: p.show_tray_icon,
        calendar_meeting_name: p.calendar_meeting_name,
      }))
      .catch(() => {});
  }, []);

  const toggle = async (key: CallKey, on: boolean) => {
    setError('');
    if (key === 'calendar_meeting_name' && on) {
      const granted = await invoke<boolean>('calendar_request_access').catch(() => false);
      if (!granted) {
        setError('Calendar access was refused. Allow Noesis in System Settings > Privacy & Security > Calendars.');
        return;
      }
    }
    setValues((v) => ({ ...v, [key]: on }));
  };

  const handleContinue = async () => {
    setSaving(true);
    setError('');
    try {
      await savePreferences(values);
      goNext();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <OnboardingContainer title="Calls" description="How Noesis behaves when a call starts." step={5} showNavigation canGoNext={false}>
      <div className="max-w-md mx-auto space-y-6">
        <div className="bg-background rounded-lg border border-border divide-y divide-border">
          {ROWS.map((r) => (
            <div key={r.key} className="flex items-center justify-between gap-4 p-4">
              <div>
                <p className="text-sm font-medium text-foreground">{r.label}</p>
                {r.hint && <p className="text-xs text-muted-foreground">{r.hint}</p>}
              </div>
              <Switch checked={values[r.key]} onCheckedChange={(on) => toggle(r.key, on)} aria-label={r.label} disabled={saving} />
            </div>
          ))}
        </div>
        <div className="space-y-2">
          <Button onClick={handleContinue} disabled={saving} className="w-full h-11">Continue</Button>
          {error && <p className="text-xs text-center text-destructive">{error}</p>}
        </div>
      </div>
    </OnboardingContainer>
  );
}
