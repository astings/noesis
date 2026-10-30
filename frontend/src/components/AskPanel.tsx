'use client';

// Live meeting ask (spec 2026-10-29-live-meeting-ask-design.md, 3.2): one input, two presets,
// the latest answer in a card. Mounted on the home page (live) and the meeting details page.
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Copy, Loader2, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';

interface AskAnswer { answer: string; from_seconds: number; to_seconds: number; segments: number }
interface AskResult { question: string; lastMinutes?: number; answer?: AskAnswer; error?: string }

const RECAP = 'Recap what has been discussed so far.';
const LAST_10 = 'What was said in the last 10 minutes?';

function mmss(seconds: number): string {
  const t = Math.max(0, Math.floor(seconds));
  return `${String(Math.floor(t / 60)).padStart(2, '0')}:${String(t % 60).padStart(2, '0')}`;
}

function errorHint(error: string): string {
  if (error.includes('API key not found')) return ' Pick one in Settings, Models.';
  if (error.startsWith('The model took too long')) return ' Try Last 10 min.';
  return '';
}

export function AskPanel({ meetingId, hasSegments, disabled }: { meetingId?: string; hasSegments: boolean; disabled?: boolean }) {
  const [question, setQuestion] = useState('');
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<AskResult | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Cmd+K (Ctrl+K elsewhere) focuses the box from anywhere on the page
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        inputRef.current?.focus();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const ask = useCallback(async (q: string, lastMinutes?: number) => {
    const text = q.trim();
    if (!text || busy) return;
    setBusy(true);
    setResult(null);
    try {
      const answer = await invoke<AskAnswer>('ask_meeting', { question: text, meetingId: meetingId ?? null, lastMinutes: lastMinutes ?? null });
      setResult({ question: text, lastMinutes, answer });
    } catch (e) {
      setResult({ question: text, lastMinutes, error: String(e) });
    } finally {
      setBusy(false);
    }
  }, [busy, meetingId]);

  const coverage = result?.answer
    ? result.lastMinutes
      ? `Last ${result.lastMinutes} min, ${mmss(result.answer.from_seconds)} to ${mmss(result.answer.to_seconds)}`
      : `Whole meeting, ${mmss(result.answer.from_seconds)} to ${mmss(result.answer.to_seconds)}, ${result.answer.segments} segments`
    : '';

  return (
    <div className="border-t border-border px-4 py-2.5 space-y-2">
      {result && (
        <div className="rounded-lg border border-border bg-card p-3 text-sm" role="status">
          <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
            <span className="truncate">{result.error ? result.question : coverage}</span>
            <div className="flex items-center gap-1">
              {result.answer && (
                <Button variant="ghost" size="sm" aria-label="Copy answer" onClick={() => navigator.clipboard.writeText(result.answer!.answer)}>
                  <Copy />
                </Button>
              )}
              <Button variant="ghost" size="sm" aria-label="Close answer" onClick={() => setResult(null)}>
                <X />
              </Button>
            </div>
          </div>
          <div className={`mt-1 whitespace-pre-wrap ${result.error ? 'text-red-600 dark:text-red-400' : ''}`}>
            {result.error ? result.error + errorHint(result.error) : result.answer!.answer}
          </div>
        </div>
      )}
      <form className="flex items-center gap-2" onSubmit={(e) => { e.preventDefault(); ask(question); }}>
        <Input
          ref={inputRef}
          aria-label="Ask about this meeting"
          className="h-8 text-[13px]"
          placeholder="Ask about this meeting… ⌘K"
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Escape') { setResult(null); e.currentTarget.blur(); } }}
          disabled={disabled || busy}
        />
        {busy ? (
          <span className="flex flex-none items-center gap-1 text-xs text-muted-foreground">
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
            Thinking…
          </span>
        ) : (
          <>
            <Button type="button" variant="outline" size="sm" className="flex-none text-[13px]" disabled={disabled || !hasSegments}
              onClick={() => { setQuestion(RECAP); ask(RECAP); }}>
              Recap so far
            </Button>
            <Button type="button" variant="outline" size="sm" className="flex-none text-[13px]" disabled={disabled || !hasSegments}
              onClick={() => { setQuestion(LAST_10); ask(LAST_10, 10); }}>
              Last 10 min
            </Button>
          </>
        )}
      </form>
    </div>
  );
}
