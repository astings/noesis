import React, { useState, useEffect } from 'react';
import { Switch } from '@/components/ui/switch';
import { Input } from '@/components/ui/input';
import { FolderOpen, Loader2 } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { DeviceSelection, SelectedDevices } from '@/components/DeviceSelection';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useConfig } from '@/contexts/ConfigContext';
import type { RawModelInfo } from '@/hooks/useTranscriptionModels';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { WikiAutomationSettings } from '@/components/WikiAutomationSettings'; // spec 4.8
import { Button } from '@/components/ui/button';
import { SettingsGroup, SettingRow } from '@/components/SettingsGroup';

export interface WikiTarget { project: string; directory: string; skill: string } // spec 4.8

export interface RecordingPreferences {
  save_folder: string;
  auto_save: boolean;
  file_format: string;
  preferred_mic_device: string | null;
  preferred_system_device: string | null;
  // spec 4.1
  auto_enhance: boolean;
  enhance_provider: 'whisper' | 'parakeet';
  enhance_model: string;
  enhance_language: string | null;
  // spec 4.5
  auto_stop_on_call_end: boolean;
  auto_start_on_call: boolean;
  owner_name: string; // spec 4.4
  post_enhance_command: string; // spec 4.7
  wiki_automation: boolean; // spec 4.8
  hook_auto_run: boolean;
  auto_title_tags: boolean;
  delete_audio_after_enhance: boolean;
  audio_retention_hours: number;
  wiki_targets: WikiTarget[];
  wiki_budget_usd: number;
}

interface RecordingSettingsProps {
  onSave?: (preferences: RecordingPreferences) => void;
}

export function RecordingSettings({ onSave }: RecordingSettingsProps) {
  const [preferences, setPreferences] = useState<RecordingPreferences>({
    save_folder: '',
    auto_save: true,
    file_format: 'mp4',
    preferred_mic_device: null,
    preferred_system_device: null,
    auto_enhance: true,
    enhance_provider: 'whisper',
    enhance_model: 'large-v3',
    enhance_language: null,
    auto_stop_on_call_end: true,
    auto_start_on_call: true,
    owner_name: 'Me',
    post_enhance_command: '',
    wiki_automation: false,
    hook_auto_run: false,
    auto_title_tags: true,
    delete_audio_after_enhance: true,
    audio_retention_hours: 24,
    wiki_targets: [],
    wiki_budget_usd: 5,
  });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [showRecordingNotification, setShowRecordingNotification] = useState(true);
  const { isRecording } = useRecordingState();
  const { setSelectedDevices } = useConfig();

  // spec 4.1: only downloaded Whisper models can be picked for the enhance job
  const [enhanceModels, setEnhanceModels] = useState<string[]>([]);
  useEffect(() => {
    invoke<RawModelInfo[]>('whisper_get_available_models')
      .then((models) => setEnhanceModels(models.filter((m) => m.status === 'Available').map((m) => m.name)))
      .catch((error) => console.error('Failed to load Whisper models:', error));
  }, []);

  const handleAutoEnhanceToggle = async (enabled: boolean) => {
    const next = { ...preferences, auto_enhance: enabled };
    setPreferences(next);
    await savePreferences(next);
  };

  // spec 4.5
  const handleAutoStopToggle = async (enabled: boolean) => {
    const next = { ...preferences, auto_stop_on_call_end: enabled };
    setPreferences(next);
    await savePreferences(next);
  };

  // spec 4.5
  const handleAutoStartToggle = async (enabled: boolean) => {
    const next = { ...preferences, auto_start_on_call: enabled };
    setPreferences(next);
    await savePreferences(next);
  };

  // spec 4.4
  const [ownerDraft, setOwnerDraft] = useState<string | null>(null);
  const commitOwnerName = async () => {
    const name = (ownerDraft ?? preferences.owner_name).trim() || 'Me';
    setOwnerDraft(null);
    if (name === preferences.owner_name) return;
    const next = { ...preferences, owner_name: name };
    setPreferences(next);
    await savePreferences(next);
  };

  // spec 4.7
  const [hookDraft, setHookDraft] = useState<string | null>(null);
  // spec 4.8
  const applyPreferences = async (next: RecordingPreferences) => {
    setPreferences(next);
    await savePreferences(next);
  };

  const [retentionDraft, setRetentionDraft] = useState<string | null>(null);
  const commitRetention = async () => {
    const hours = Math.max(1, Math.round(Number(retentionDraft ?? preferences.audio_retention_hours)) || 1);
    setRetentionDraft(null);
    if (hours === preferences.audio_retention_hours) return;
    await applyPreferences({ ...preferences, audio_retention_hours: hours });
  };

  const [taggingAll, setTaggingAll] = useState(false);
  const tagUntagged = async () => {
    setTaggingAll(true);
    try {
      toast.success(await invoke<string>('tag_untagged_meetings'));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setTaggingAll(false);
    }
  };

  const commitHookCommand = async () => {
    const command = (hookDraft ?? preferences.post_enhance_command).trim();
    setHookDraft(null);
    if (command === preferences.post_enhance_command) return;
    const next = { ...preferences, post_enhance_command: command };
    setPreferences(next);
    await savePreferences(next);
  };

  const handleEnhanceModelChange = async (model: string) => {
    const next = { ...preferences, enhance_provider: 'whisper' as const, enhance_model: model };
    setPreferences(next);
    await savePreferences(next);
  };

  // Load recording preferences on component mount
  useEffect(() => {
    const loadPreferences = async () => {
      try {
        const prefs = await invoke<RecordingPreferences>('get_recording_preferences');
        setPreferences(prefs);
      } catch (error) {
        console.error('Failed to load recording preferences:', error);
        // If loading fails, get default folder path
        try {
          const defaultPath = await invoke<string>('get_default_recordings_folder_path');
          setPreferences(prev => ({ ...prev, save_folder: defaultPath }));
        } catch (defaultError) {
          console.error('Failed to get default folder path:', defaultError);
        }
      } finally {
        setLoading(false);
      }
    };

    loadPreferences();
  }, []);

  // Load recording notification preference
  useEffect(() => {
    const loadNotificationPref = async () => {
      try {
        const { Store } = await import('@tauri-apps/plugin-store');
        const store = await Store.load('preferences.json');
        const show = await store.get<boolean>('show_recording_notification') ?? true;
        setShowRecordingNotification(show);
      } catch (error) {
        console.error('Failed to load notification preference:', error);
      }
    };
    loadNotificationPref();
  }, []);

  const handleAutoSaveToggle = async (enabled: boolean) => {
    const newPreferences = { ...preferences, auto_save: enabled };
    setPreferences(newPreferences);
    await savePreferences(newPreferences);
  };

  const handleDeviceChange = async (devices: SelectedDevices) => {
    const newPreferences = {
      ...preferences,
      preferred_mic_device: devices.micDevice,
      preferred_system_device: devices.systemDevice
    };
    setPreferences(newPreferences);
    // Sync the in-memory selection ConfigContext exposes to the start path.
    // Without this, the picker only writes to disk (loaded into context once at
    // app mount), so a newly-chosen mic isn't honored until the next launch —
    // start keeps sending the stale launch-time device.
    setSelectedDevices(devices);
    await savePreferences(newPreferences);
  };

  const handleOpenFolder = async () => {
    try {
      await invoke('open_recordings_folder');
    } catch (error) {
      console.error('Failed to open recordings folder:', error);
    }
  };

  const handleNotificationToggle = async (enabled: boolean) => {
    try {
      setShowRecordingNotification(enabled);
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('preferences.json');
      await store.set('show_recording_notification', enabled);
      await store.save();
      toast.success('Preference saved');
    } catch (error) {
      console.error('Failed to save notification preference:', error);
      toast.error('Failed to save preference');
    }
  };

  const savePreferences = async (prefs: RecordingPreferences) => {
    setSaving(true);
    try {
      await invoke('set_recording_preferences', { preferences: prefs });
      onSave?.(prefs);

      // Show success toast with device details
      const micDevice = prefs.preferred_mic_device || 'Default';
      const systemDevice = prefs.preferred_system_device || 'Default';
      toast.success("Device preferences saved", {
        description: `Microphone: ${micDevice}, System Audio: ${systemDevice}`
      });
    } catch (error) {
      console.error('Failed to save recording preferences:', error);
      toast.error("Failed to save device preferences", {
        description: error instanceof Error ? error.message : String(error)
      });
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <div className="animate-pulse">
        <div className="h-4 bg-accent rounded w-1/4 mb-4"></div>
        <div className="h-8 bg-accent rounded mb-4"></div>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <SettingsGroup>
        <SettingRow
          label="Save audio recordings"
          hint="Save the audio file when a recording stops"
          control={<Switch checked={preferences.auto_save} onCheckedChange={handleAutoSaveToggle} disabled={saving} />}
        />
        {preferences.auto_save ? (
          <SettingRow
            label="Recordings folder"
            hint={
              <span className="font-mono break-all">
                {preferences.save_folder || 'Default folder'} · {preferences.file_format.toUpperCase()}
              </span>
            }
            control={
              <Button variant="outline" size="sm" onClick={handleOpenFolder}>
                <FolderOpen />
                Open folder
              </Button>
            }
          />
        ) : (
          <div className="px-3.5 py-3 text-xs text-warning">
            Audio recording is off. Turn on "Save audio recordings" to keep the meeting audio.
          </div>
        )}
        {/* spec 4.1: enhance after recording */}
        <SettingRow
          label="Enhance transcript after recording"
          hint="Re-transcribe the full recording with a slower, more accurate model as soon as it stops"
          control={<Switch checked={preferences.auto_enhance} onCheckedChange={handleAutoEnhanceToggle} disabled={saving} />}
        >
          {preferences.auto_enhance && (
            <div className="flex flex-wrap items-center gap-3">
              <span className="text-[13px] text-foreground">Model</span>
              <Select value={preferences.enhance_model} onValueChange={handleEnhanceModelChange} disabled={saving}>
                <SelectTrigger className="h-8 w-64 text-[13px]">
                  <SelectValue placeholder="Pick a downloaded Whisper model" />
                </SelectTrigger>
                <SelectContent>
                  {enhanceModels.map((name) => (
                    <SelectItem key={name} value={name}>{name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {enhanceModels.length === 0 && (
                <span className="text-xs text-warning">
                  No Whisper model downloaded yet. Download one in Transcription settings.
                </span>
              )}
            </div>
          )}
        </SettingRow>
        <SettingRow
          label="Recording start notification"
          hint="Remind you to tell participants that the meeting is recorded"
          control={<Switch checked={showRecordingNotification} onCheckedChange={handleNotificationToggle} />}
        />
      </SettingsGroup>

      {/* spec 4.5: Zoom call detection */}
      <SettingsGroup title="Zoom">
        <SettingRow
          label="Start recording when a Zoom call begins"
          hint="Records into the project selected on the home screen"
          control={<Switch checked={preferences.auto_start_on_call} onCheckedChange={handleAutoStartToggle} disabled={saving} />}
        />
        <SettingRow
          label="Stop recording when the Zoom call ends"
          hint="About 20 seconds after you leave the meeting, the recording stops and is saved"
          control={<Switch checked={preferences.auto_stop_on_call_end} onCheckedChange={handleAutoStopToggle} disabled={saving} />}
        />
      </SettingsGroup>

      {/* spec 4.4: speaker labels */}
      <SettingsGroup title="Transcripts">
        <SettingRow
          label="Your name in transcripts"
          hint='Lines from your microphone are labelled with this name; everyone else is "Others"'
          control={
            <Input
              aria-label="Your name in transcripts"
              className="h-8 w-44 text-[13px]"
              value={ownerDraft ?? preferences.owner_name}
              onChange={(e) => setOwnerDraft(e.target.value)}
              onBlur={commitOwnerName}
              onKeyDown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur(); }}
              disabled={saving}
            />
          }
        />
      </SettingsGroup>

      {/* spec 4.8: wiki automation */}
      <WikiAutomationSettings preferences={preferences} onChange={applyPreferences} disabled={saving} />

      <SettingsGroup title="Titles, tags and audio">
        <SettingRow
          label="Name and tag meetings after enhance"
          hint="Uses your summary model. Off: run it from the meeting page or the right-click menu."
          control={<Switch aria-label="Name and tag meetings after enhance" checked={preferences.auto_title_tags} onCheckedChange={(on) => applyPreferences({ ...preferences, auto_title_tags: on })} disabled={saving} />}
        />
        <SettingRow
          label="Tag meetings without tags"
          control={
            <Button size="sm" variant="outline" onClick={tagUntagged} disabled={taggingAll}>
              {taggingAll && <Loader2 className="animate-spin" />}
              Tag meetings without tags
            </Button>
          }
        />
        <SettingRow
          label="Delete audio after enhance"
          hint="The transcript stays. Enhance and playback stop working for that meeting."
          control={<Switch aria-label="Delete audio after enhance" checked={preferences.delete_audio_after_enhance} onCheckedChange={(on) => applyPreferences({ ...preferences, delete_audio_after_enhance: on })} disabled={saving} />}
        />
        <SettingRow
          label="Keep audio for (hours)"
          control={
            <Input
              type="number"
              min={1}
              step={1}
              aria-label="Keep audio for (hours)"
              className="h-8 w-24"
              value={retentionDraft ?? preferences.audio_retention_hours}
              onChange={(e) => setRetentionDraft(e.target.value)}
              onBlur={commitRetention}
              onKeyDown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur(); }}
              disabled={saving}
            />
          }
        />
      </SettingsGroup>

      {/* spec 4.7: post-enhance hook */}
      <SettingsGroup title="Advanced">
        <SettingRow
          label="Run hook automatically after enhance"
          hint="Off: run it from the meeting page or the right-click menu."
          control={<Switch aria-label="Run hook automatically after enhance" checked={preferences.hook_auto_run} onCheckedChange={(on) => applyPreferences({ ...preferences, hook_auto_run: on })} disabled={saving} />}
        />
        <SettingRow
          label="Custom command"
          hint="Shell command started after an enhance when wiki automation is off or the project has no wiki. It gets NOESIS_FOLDER, NOESIS_PROJECT, NOESIS_TITLE and NOESIS_MEETING_ID (the MEETILY_* names still work); output goes to hook.log in the meeting folder. Empty means off."
        >
          <Input
            aria-label="Custom command (advanced)"
            className="h-8 font-mono text-xs"
            placeholder="~/Code/Perso/ok-meetily/scripts/post-enhance.sh"
            value={hookDraft ?? preferences.post_enhance_command}
            onChange={(e) => setHookDraft(e.target.value)}
            onBlur={commitHookCommand}
            onKeyDown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur(); }}
            disabled={saving}
          />
        </SettingRow>
      </SettingsGroup>

      {/* Device Preferences */}
      <SettingsGroup title="Default audio devices">
        <SettingRow
          label="Microphone and system audio"
          hint="Selected automatically when a new recording starts"
        >
          {isRecording && (
            <p
              role="status"
              aria-live="polite"
              className="text-xs text-warning bg-warning/10 border border-warning/40 rounded-md p-2"
            >
              Device selection is locked while a recording is in progress. Connecting a new device mid-recording will not switch to it. Stop the current meeting to change devices.
            </p>
          )}
          <DeviceSelection
            selectedDevices={{
              micDevice: preferences.preferred_mic_device,
              systemDevice: preferences.preferred_system_device
            }}
            onDeviceChange={handleDeviceChange}
            disabled={saving || isRecording}
          />
        </SettingRow>
      </SettingsGroup>
    </div>
  );
}