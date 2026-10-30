import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ChevronRight } from 'lucide-react';
import { INBOX } from '@/lib/projects';
import { languageSettingLabel, providerName } from '@/lib/languages';
import { EMPTY_PROJECT_SETTINGS, ProjectSettingsDialog, type ProjectSettings } from '@/components/ProjectSettingsDialog';

const summary = (s: ProjectSettings) => {
  const parts = [
    s.language && languageSettingLabel(s.language),
    s.enhance_provider && s.enhance_model && `${providerName(s.enhance_provider)} ${s.enhance_model}`,
  ].filter(Boolean);
  return parts.length ? parts.join(' · ') : 'Defaults';
};

/** One row per project with its overrides; a click opens the project settings dialog. */
export function ProjectOverrides() {
  const [projects, setProjects] = useState<string[]>([]);
  const [settings, setSettings] = useState<Record<string, ProjectSettings>>({});
  const [editing, setEditing] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const list = await invoke<string[]>('list_projects_command').catch(() => [INBOX]);
      const entries = await Promise.all(list.map(async (p) => [p, await invoke<ProjectSettings>('get_project_settings', { project: p }).catch(() => EMPTY_PROJECT_SETTINGS)] as const));
      if (cancelled) return;
      setProjects(list);
      setSettings(Object.fromEntries(entries));
    })();
    return () => { cancelled = true; };
  }, []);

  if (!projects.length) return null;
  return (
    <>
      <ul className="w-full divide-y divide-border rounded-md border border-border text-[13px]">
        {projects.map((p) => {
          const s = settings[p] ?? EMPTY_PROJECT_SETTINGS;
          return (
            <li key={p}>
              <button type="button" onClick={() => setEditing(p)} className="flex w-full items-center gap-3 px-3 py-2 text-left hover:bg-accent/60">
                <span className="flex-1 truncate">{p === INBOX ? 'Inbox' : p}</span>
                <span className={`truncate ${summary(s) === 'Defaults' ? 'text-muted-foreground' : ''}`}>{summary(s)}</span>
                <ChevronRight className="h-3.5 w-3.5 flex-none text-muted-foreground" />
              </button>
            </li>
          );
        })}
      </ul>
      <ProjectSettingsDialog
        project={editing}
        onOpenChange={(open) => { if (!open) setEditing(null); }}
        onSaved={(p, next) => setSettings((all) => ({ ...all, [p]: next }))}
      />
    </>
  );
}
