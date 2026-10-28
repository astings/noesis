'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Switch } from '@/components/ui/switch';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { ProjectSelect } from '@/components/ProjectSelect';
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
    <div className="p-4 border rounded-lg space-y-3">
      <div className="flex items-center justify-between gap-4">
        <div>
          <div className="font-medium">Wiki automation</div>
          <div className="text-sm text-gray-600">
            After each enhance, run Claude Code in the project&apos;s wiki to draft the transcript document and a log entry. Nothing is committed, scored or moved.
          </div>
        </div>
        <Switch
          checked={preferences.wiki_automation}
          onCheckedChange={(on) => onChange({ ...preferences, wiki_automation: on })}
          disabled={disabled}
          aria-label="Wiki automation"
        />
      </div>
      <div className="text-xs text-gray-500">
        {claudePath === undefined
          ? 'Looking for Claude Code...'
          : claudePath
            ? `Claude Code: ${claudePath}`
            : 'Claude Code not found. Install it and sign in, then reopen settings.'}
      </div>

      <div className="space-y-2">
        {rows.map((row, i) => (
          <div key={i} className="grid grid-cols-[10rem_1fr_10rem_auto] gap-2 items-center">
            <ProjectSelect compact value={row.project} onChange={(p) => commitRows(updateRow(i, { project: p }))} disabled={disabled} />
            <div className="flex gap-1">
              <Input
                aria-label="Wiki directory"
                className="font-mono text-xs"
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
              className="text-xs"
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

      <div className="flex items-center gap-2">
        <div className="text-sm text-gray-600">Cost cap per run (USD)</div>
        <Input
          aria-label="Cost cap per run in USD"
          className="w-24"
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
      </div>
    </div>
  );
}
