'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { INBOX, normalizeProjectName } from '@/lib/projects';

const NEW_PROJECT = '__new__';

interface ProjectSelectProps {
  value: string;
  onChange: (project: string) => void;
  disabled?: boolean;
  compact?: boolean;
}

/** spec 4.2: pick a project folder. Projects are folders under the recordings root; "New project" creates one by name. */
export function ProjectSelect({ value, onChange, disabled, compact }: ProjectSelectProps) {
  const [projects, setProjects] = useState<string[]>([INBOX]);
  const [creating, setCreating] = useState(false);
  const [draft, setDraft] = useState('');

  useEffect(() => {
    invoke<string[]>('list_projects_command')
      .then((list) => setProjects(list.length ? list : [INBOX]))
      .catch((error) => console.warn('[ProjectSelect] list_projects_command failed:', error));
  }, []);

  const options = !value || projects.includes(value) ? projects : [...projects, value]; // spec 4.8: an unset value must not become an empty item

  const confirmNew = () => {
    const name = normalizeProjectName(draft);
    if (!name) return;
    setProjects((prev) => (prev.includes(name) ? prev : [...prev, name].sort((a, b) => (a === INBOX ? -1 : b === INBOX ? 1 : a.localeCompare(b)))));
    setCreating(false);
    setDraft('');
    onChange(name);
  };

  if (creating) {
    return (
      <div className="flex items-center gap-1">
        <Input
          autoFocus
          value={draft}
          placeholder="project name"
          className={compact ? 'h-8 w-36 text-xs' : 'h-9 w-44 text-sm'}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') confirmNew();
            if (e.key === 'Escape') { setCreating(false); setDraft(''); }
          }}
        />
        <Button size="sm" variant="outline" onClick={confirmNew} disabled={!normalizeProjectName(draft)}>Add</Button>
      </div>
    );
  }

  return (
    <Select
      value={value}
      disabled={disabled}
      onValueChange={(v) => {
        if (v === NEW_PROJECT) { setCreating(true); return; }
        onChange(v);
      }}
    >
      <SelectTrigger className={compact ? 'h-8 w-36 text-xs' : 'h-9 w-44 text-sm'} title="Project folder for this meeting">
        <SelectValue placeholder="Project" />
      </SelectTrigger>
      <SelectContent>
        {options.map((p) => (
          <SelectItem key={p} value={p}>{p === INBOX ? 'Inbox' : p}</SelectItem>
        ))}
        <SelectItem value={NEW_PROJECT}>New project...</SelectItem>
      </SelectContent>
    </Select>
  );
}
