import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type JobState = 'queued' | 'running' | 'done' | 'failed' | 'cancelled';
export interface Job {
  id: string;
  kind: 'enhance' | 'summary' | 'download' | 'tagging';
  meeting_id: string | null;
  title: string;
  state: JobState;
  steps: { name: string; state: JobState; error: string | null }[];
  progress: number | null;
  detail: string | null;
  eta_secs: number | null;
  started_at: string | null;
  finished_at: string | null;
  error: string | null;
}

export const isTerminal = (s: JobState) => s === 'done' || s === 'failed' || s === 'cancelled';

/** Background jobs from the Rust registry (jobs.rs). Refetches on focus so a dropped event heals. */
export function useJobs() {
  const [jobs, setJobs] = useState<Job[]>([]);
  useEffect(() => {
    const refresh = () => invoke<Job[]>('list_jobs').then(setJobs).catch(() => {});
    refresh();
    const unlisten = listen<Job[]>('jobs-changed', (e) => setJobs(e.payload));
    window.addEventListener('focus', refresh);
    return () => {
      unlisten.then((f) => f());
      window.removeEventListener('focus', refresh);
    };
  }, []);
  return jobs;
}
