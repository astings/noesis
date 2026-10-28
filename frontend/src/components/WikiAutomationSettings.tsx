'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Switch } from '@/components/ui/switch';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { ProjectSelect } from '@/components/ProjectSelect';
import { SettingsGroup, SettingRow } from '@/components/SettingsGroup';
import type { RecordingPreferences, WikiTarget } from '@/components/RecordingSettings';

interface Props {
  preferences: RecordingPreferences;
  onChange: (next: RecordingPreferences) => Promise<void> | void;
  disabled?: boolean;
}

/** spec 4.8: one wiki per project; the app runs Claude Code there after each enhance. */
export function WikiAutomationSettings({ preferences, onChange, disabled }: Props) {
  const [claudePath, setClaudePath] = useState<string | null | undefined>(undefined);
  const [draft, setDraft] = useState<WikiTarget[] | null>(null);
  const [budgetDraft, setBudgetDraft] = useState<string | null>(null);

  useEffect(() => {
    invoke<string | null>('locate_claude_cli').then(setClaudePath).catch(() => setClaudePath(null));
  }, []);

  const rows = draft ?? preferences.wiki_targets;
  const commitRows = async (next: WikiTarget[]) => {
    setDraft(null);
    await onChange({ ...preferences, wiki_targets: next });
  };
  const updateRow = (i: number, patch: Partial<WikiTarget>) => {
    const next = rows.map((r, j) => (j === i ? { ...r, ...patch } : r));
    setDraft(next);
    return next;
  };
  const browse = async (i: number) => {
    const picked = await invoke<string | null>('select_wiki_directory').catch(() => null);
    if (picked) await commitRows(updateRow(i, { directory: picked }));
  };
  const commitBudget = async () => {
    const value = parseFloat(budgetDraft ?? String(preferences.wiki_budget_usd));
    setBudgetDraft(null);
    if (!Number.isFinite(value) || value <= 0 || value === preferences.wiki_budget_usd) return;
    await onChange({ ...preferences, wiki_budget_usd: value });
  };

  return (
    <SettingsGroup title="Wiki automation">
      <SettingRow
        label="Run Claude after enhance"
        hint={
          <>
            After each enhance, run Claude Code in the project&apos;s wiki to draft the transcript document and a log entry. Nothing is committed, scored or moved.
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
            checked={preferences.wiki_automation}
            onCheckedChange={(on) => onChange({ ...preferences, wiki_automation: on })}
            disabled={disabled}
            aria-label="Wiki automation"
          />
        }
      />

      <SettingRow label="Project wikis" hint="One wiki directory per project, with an optional skill">
        <div className="space-y-2">
          {rows.map((row, i) => (
            <div key={i} className="grid grid-cols-[9rem_1fr_8rem_auto] gap-2 items-center">
              <ProjectSelect compact value={row.project} onChange={(p) => commitRows(updateRow(i, { project: p }))} disabled={disabled} />
              <div className="flex gap-1">
                <Input
                  aria-label="Wiki directory"
                  className="h-8 font-mono text-xs"
                  placeholder="/Users/you/Code/wiki"
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
                aria-label="Skill (optional)"
                className="h-8 text-xs"
                placeholder="skill (optional)"
                value={row.skill}
                onChange={(e) => updateRow(i, { skill: e.target.value })}
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
            onClick={() => commitRows([...rows, { project: '', directory: '', skill: '' }])}
            disabled={disabled}
          >
            Add project
          </Button>
        </div>
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
            value={budgetDraft ?? String(preferences.wiki_budget_usd)}
            onChange={(e) => setBudgetDraft(e.target.value)}
            onBlur={commitBudget}
            onKeyDown={(e) => {
              if (e.key === 'Enter') e.currentTarget.blur();
            }}
            disabled={disabled}
          />
        }
      />
    </SettingsGroup>
  );
}
