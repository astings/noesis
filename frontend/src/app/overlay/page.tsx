'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { LogicalPosition } from '@tauri-apps/api/dpi';
import { Pause, Play, Square, X } from 'lucide-react';
import { ProjectSelect } from '@/components/ProjectSelect';

const PILL_HEIGHT = 48;
const ASK_HEIGHT = 360;
const SELECT_HEIGHT = 240; // room for the project dropdown, which portals inside the window

interface AskAnswer { answer: string }

const resize = (height: number) => invoke('overlay_resize', { height }).catch(() => {});

function formatTime(total: number) {
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = Math.floor(total % 60);
  const mm = String(m).padStart(h ? 2 : 1, '0');
  return `${h ? `${h}:` : ''}${mm}:${String(s).padStart(2, '0')}`;
}

/** Call overlay: a small always-on-top pill with the live recording controls. */
export default function OverlayPage() {
  const [seconds, setSeconds] = useState(0);
  const [paused, setPaused] = useState(false);
  const [project, setProject] = useState('');
  const [question, setQuestion] = useState('');
  const [loading, setLoading] = useState(false);
  const [answer, setAnswer] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const answerOpen = useRef(false);
  const askId = useRef(0); // bumped on each ask and on close, so a stale result is ignored

  const fail = (e: unknown) => { answerOpen.current = true; setError(String(e)); resize(ASK_HEIGHT); };

  useEffect(() => {
    invoke<{ is_paused: boolean; active_duration: number | null }>('get_recording_state')
      .then((s) => { setPaused(s.is_paused); setSeconds(Math.floor(s.active_duration ?? 0)); })
      .catch(fail);
    invoke<string>('get_recording_project').then(setProject).catch(fail);

    const unlisten = [
      listen('recording-paused', () => setPaused(true)),
      listen('recording-resumed', () => setPaused(false)),
      listen<string>('recording-project-changed', (e) => setProject(e.payload)),
    ];

    try {
      const saved = JSON.parse(localStorage.getItem('overlayPos') ?? 'null');
      if (saved) getCurrentWindow().setPosition(new LogicalPosition(saved.x, saved.y)).catch(() => {});
    } catch { /* no saved position */ }
    const moved = getCurrentWindow().onMoved(async ({ payload }) => {
      try {
        const pos = payload.toLogical(await getCurrentWindow().scaleFactor());
        localStorage.setItem('overlayPos', JSON.stringify({ x: pos.x, y: pos.y }));
      } catch { /* position not persisted */ }
    });

    return () => {
      unlisten.forEach((u) => u.then((fn) => fn()));
      moved.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (paused) return;
    const id = setInterval(() => setSeconds((s) => s + 1), 1000);
    return () => clearInterval(id);
  }, [paused]);

  const closeAnswer = useCallback(() => {
    askId.current++;
    answerOpen.current = false;
    setAnswer(null);
    setError(null);
    setLoading(false);
    resize(PILL_HEIGHT);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape' && answerOpen.current) closeAnswer(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [closeAnswer]);

  const ask = async () => {
    const text = question.trim();
    if (!text || loading) return;
    const id = ++askId.current;
    answerOpen.current = true;
    setLoading(true);
    setAnswer(null);
    setError(null);
    resize(ASK_HEIGHT);
    try {
      const r = await invoke<AskAnswer>('ask_meeting', { question: text, meetingId: null, lastMinutes: null });
      if (id === askId.current) setAnswer(r.answer);
    } catch (e) {
      if (id === askId.current) fail(e);
    } finally {
      if (id === askId.current) setLoading(false);
    }
  };

  const togglePause = () =>
    invoke(paused ? 'resume_recording' : 'pause_recording').catch(fail);

  const changeProject = (p: string) =>
    invoke<string>('set_recording_project', { project: p }).then(setProject).catch(fail);

  const iconBtn = 'flex h-7 w-7 shrink-0 items-center justify-center rounded-full hover:bg-white/15';
  const showPanel = loading || answer !== null || error !== null;

  return (
    <div className="flex h-screen w-screen flex-col gap-1 bg-transparent">
      <div
        data-tauri-drag-region
        className="flex h-10 shrink-0 items-center gap-2 rounded-full bg-neutral-900/90 px-3 text-white shadow-lg"
      >
        <span className={`h-2.5 w-2.5 shrink-0 rounded-full bg-red-500 ${paused ? 'opacity-50' : 'animate-pulse'}`} />
        <span data-tauri-drag-region className="w-12 shrink-0 text-xs tabular-nums">{formatTime(seconds)}</span>
        <button className={iconBtn} onClick={togglePause} title={paused ? 'Resume' : 'Pause'}>
          {paused ? <Play size={14} /> : <Pause size={14} />}
        </button>
        <button className={`${iconBtn} text-red-400`} onClick={() => invoke('overlay_stop_recording').catch(fail)} title="Stop recording">
          <Square size={14} fill="currentColor" />
        </button>
        <ProjectSelect
          compact
          value={project}
          onChange={changeProject}
          onOpenChange={(open) => { if (!answerOpen.current) resize(open ? SELECT_HEIGHT : PILL_HEIGHT); }}
          className="h-7 w-28 border-white/20 bg-transparent text-white"
        />
        <input
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') ask(); }}
          placeholder="Ask…"
          className="h-7 min-w-0 flex-1 rounded-full bg-white/10 px-3 text-xs text-white placeholder:text-white/50 outline-none"
        />
      </div>
      {showPanel && (
        <div className="relative min-h-0 flex-1 overflow-y-auto rounded-2xl bg-neutral-900/90 p-3 pr-8 text-xs text-white shadow-lg">
          <button className="absolute right-2 top-2 rounded-full p-1 hover:bg-white/15" onClick={closeAnswer} title="Close">
            <X size={12} />
          </button>
          {loading && <p className="text-white/60">Thinking…</p>}
          {answer !== null && <p className="whitespace-pre-wrap">{answer}</p>}
          {error !== null && <p className="text-red-400">{error}</p>}
        </div>
      )}
    </div>
  );
}
