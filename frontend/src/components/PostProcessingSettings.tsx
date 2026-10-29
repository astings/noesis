'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Switch } from '@/components/ui/switch';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { ProjectSelect } from '@/components/ProjectSelect';
import { SettingsGroup, SettingRow } from '@/components/SettingsGroup';
import type { RecordingPreferences, PostProcessingTarget } from '@/components/RecordingSettings';

interface Props {
  preferences: RecordingPreferences;
  onChange: (next: RecordingPreferences) => Promise<void> | void;
  disabled?: boolean;
}

const splitTools = (text: string) => text.split(/[\s,]+/).map((s) => s.trim()).filter(Boolean);

/** Generic settings section 1: one directory per project; the app runs Claude Code there after each enhance. */
export function PostProcessingSettings({ preferences, onChange, disabled }: Props) {
  const [claudePath, setClaudePath] = useState<string | null | undefined>(undefined);
  const [defaultTemplate, setDefaultTemplate] = useState('');
  const [draft, setDraft] = useState<PostProcessingTarget[] | null>(null);
  const [budgetDraft, setBudgetDraft] = useState<string | null>(null);
  const [agentDraft, setAgentDraft] = useState<string | null>(null);
  const [templateDraft, setTemplateDraft] = useState<string | null>(null);
  const [allowedDraft, setAllowedDraft] = useState<string | null>(null);
  const [deniedDraft, setDeniedDraft] = useState<string | null>(null);

  useEffect(() => {
    invoke<string | null>('locate_claude_cli').then(setClaudePath).catch(() => setClaudePath(null));
    invoke<string>('get_default_agent_prompt_template').then(setDefaultTemplate).catch(() => setDefaultTemplate(''));
  }, []);

  const rows = draft ?? preferences.post_processing_targets;
  const commitRows = async (next: PostProcessingTarget[]) => {
    setDraft(null);
    await onChange({ ...preferences, post_processing_targets: next });
  };
  const updateRow = (i: number, patch: Partial<PostProcessingTarget>) => {
    const next = rows.map((r, j) => (j === i ? { ...r, ...patch } : r));
    setDraft(next);
    return next;
  };
  const browse = async (i: number) => {
    const picked = await invoke<string | null>('select_directory').catch(() => null);
    if (picked) await commitRows(updateRow(i, { directory: picked }));
  };
  const commitBudget = async () => {
    const value = parseFloat(budgetDraft ?? String(preferences.post_processing_budget_usd));
    setBudgetDraft(null);
    if (!Number.isFinite(value) || value <= 0 || value === preferences.post_processing_budget_usd) return;
    await onChange({ ...preferences, post_processing_budget_usd: value });
  };
  const commitText = async (key: 'agent_path' | 'agent_prompt_template', value: string, reset: () => void) => {
    reset();
    if (value.trim() === preferences[key]) return;
    await onChange({ ...preferences, [key]: value.trim() });
  };
  const commitTools = async (key: 'agent_allowed_tools' | 'agent_denied_tools', text: string, reset: () => void) => {
    reset();
    const list = splitTools(text);
    if (list.join(' ') === preferences[key].join(' ')) return;
    await onChange({ ...preferences, [key]: list });
  };
  const blurOnEnter = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') e.currentTarget.blur();
  };

  return (
    <SettingsGroup title="Post-processing">
      <SettingRow
        label="Run Claude Code after enhance"
        hint={
          <>
            After each enhance, run Claude Code in the project&apos;s directory with the prompt below. Output goes to hook.log in the meeting folder.
            <span className="block mt-1 font-mono">
              {claudePath === undefined
                ? 'Looking for Claude Code...'
                : claudePath
                  ? `Claude Code: ${claudePath}`
                  : 'Claude Code not found. Install it and sign in, then reopen settings.'}
            </span>
          </>
        }
        control={
          <Switch
            checked={preferences.post_processing}
            onCheckedChange={(on) => onChange({ ...preferences, post_processing: on })}
            disabled={disabled}
            aria-label="Post-processing"
          />
        }
      />

      <SettingRow label="Claude Code path" hint="Leave empty to use the detected one">
        <Input
          aria-label="Claude Code path"
          className="h-8 font-mono text-xs"
          placeholder={claudePath ?? '/path/to/claude'}
          value={agentDraft ?? preferences.agent_path}
          onChange={(e) => setAgentDraft(e.target.value)}
          onBlur={() => commitText('agent_path', agentDraft ?? preferences.agent_path, () => setAgentDraft(null))}
          onKeyDown={blurOnEnter}
          disabled={disabled}
        />
      </SettingRow>

      <SettingRow label="Projects" hint="One directory per project, with optional instructions added to the prompt">
        <div className="space-y-2">
          {rows.map((row, i) => (
            <div key={i} className="grid grid-cols-[9rem_1fr_12rem_auto] gap-2 items-center">
              <ProjectSelect compact value={row.project} onChange={(p) => commitRows(updateRow(i, { project: p }))} disabled={disabled} />
              <div className="flex gap-1">
                <Input
                  aria-label="Directory"
                  className="h-8 font-mono text-xs"
                  placeholder="/path/to/project-notes"
                  value={row.directory}
                  onChange={(e) => updateRow(i, { directory: e.target.value })}
                  onBlur={() => draft && commitRows(draft)}
                  disabled={disabled}
                />
                <Button type="button" variant="outline" size="sm" onClick={() => browse(i)} disabled={disabled}>
                  Browse
                </Button>
              </div>
              <Input
                aria-label="Instructions (optional)"
                className="h-8 text-xs"
                placeholder="Optional, e.g. Use the dex-case-tracking skill."
                value={row.instructions}
                onChange={(e) => updateRow(i, { instructions: e.target.value })}
                onBlur={() => draft && commitRows(draft)}
                disabled={disabled}
              />
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => commitRows(rows.filter((_, j) => j !== i))}
                disabled={disabled}
                aria-label="Remove row"
              >
                Remove
              </Button>
            </div>
          ))}
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => commitRows([...rows, { project: '', directory: '', instructions: '' }])}
            disabled={disabled}
          >
            Add project
          </Button>
        </div>
      </SettingRow>

      <SettingRow
        label="Prompt template"
        hint="Placeholders: {folder} {project} {title} {meeting_id} {instructions}. Empty means the built-in template."
        control={
          <Button
            size="sm"
            variant="ghost"
            disabled={disabled || !preferences.agent_prompt_template}
            onClick={() => { setTemplateDraft(null); onChange({ ...preferences, agent_prompt_template: '' }); }}
          >
            Reset
          </Button>
        }
      >
        <Textarea
          aria-label="Prompt template"
          rows={4}
          className="font-mono text-xs"
          value={templateDraft ?? (preferences.agent_prompt_template || defaultTemplate)}
          onChange={(e) => setTemplateDraft(e.target.value)}
          onBlur={() => {
            const value = templateDraft ?? preferences.agent_prompt_template;
            // the built-in text typed back verbatim stays "empty"
            commitText('agent_prompt_template', value.trim() === defaultTemplate.trim() ? '' : value, () => setTemplateDraft(null));
          }}
          disabled={disabled}
        />
      </SettingRow>

      <SettingRow label="Allowed tools" hint="Space or comma separated. Empty means file edits only.">
        <Input
          aria-label="Allowed tools"
          className="h-8 font-mono text-xs"
          value={allowedDraft ?? preferences.agent_allowed_tools.join(' ')}
          onChange={(e) => setAllowedDraft(e.target.value)}
          onBlur={() => commitTools('agent_allowed_tools', allowedDraft ?? preferences.agent_allowed_tools.join(' '), () => setAllowedDraft(null))}
          onKeyDown={blurOnEnter}
          disabled={disabled}
        />
      </SettingRow>

      <SettingRow label="Denied tools" hint="Space or comma separated">
        <Input
          aria-label="Denied tools"
          className="h-8 font-mono text-xs"
          value={deniedDraft ?? preferences.agent_denied_tools.join(' ')}
          onChange={(e) => setDeniedDraft(e.target.value)}
          onBlur={() => commitTools('agent_denied_tools', deniedDraft ?? preferences.agent_denied_tools.join(' '), () => setDeniedDraft(null))}
          onKeyDown={blurOnEnter}
          disabled={disabled}
        />
      </SettingRow>

      <SettingRow
        label="Cost cap per run"
        hint="In US dollars, per Claude Code run"
        control={
          <Input
            aria-label="Cost cap per run in USD"
            className="h-8 w-24 text-[13px]"
            type="number"
            min={0.5}
            step={0.5}
            value={budgetDraft ?? String(preferences.post_processing_budget_usd)}
            onChange={(e) => setBudgetDraft(e.target.value)}
            onBlur={commitBudget}
            onKeyDown={blurOnEnter}
            disabled={disabled}
          />
        }
      />
    </SettingsGroup>
  );
}
