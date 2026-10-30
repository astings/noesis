import React, { useEffect, useState, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Mic, Check, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { toast } from 'sonner';
import type { ParakeetDownloadProgressEvent } from '@/lib/parakeet';
import type { RecordingPreferences } from '@/components/RecordingSettings';

const PARAKEET_MODEL = 'parakeet-tdt-0.6b-v3-int8';

const ENHANCE_MODELS = [
  { name: 'small', label: 'Small', size: '~470 MB', hint: 'Fastest' },
  { name: 'medium', label: 'Medium', size: '~1.5 GB', hint: 'Balanced' },
  { name: 'large-v3', label: 'Large', size: '~3 GB', hint: 'Most accurate' },
] as const;

type DownloadStatus = 'waiting' | 'downloading' | 'completed' | 'cancelled' | 'error';

interface DownloadState {
  status: DownloadStatus;
  progress: number;
  downloadedMb: number;
  totalMb: number;
  speedMbps: number;
  error?: string;
}

export function DownloadProgressStep() {
  const {
    goToStep,
    isMac,
    parakeetError,
    parakeetDownloaded,
    setParakeetDownloaded,
    startBackgroundDownloads,
    savePreferences,
  } = useOnboarding();

  const [autoEnhance, setAutoEnhance] = useState(true);
  const [enhanceModel, setEnhanceModel] = useState<string>('large-v3');
  const [saveError, setSaveError] = useState('');
  const [continuing, setContinuing] = useState(false);

  useEffect(() => {
    invoke<RecordingPreferences>('get_recording_preferences')
      .then((p) => {
        setAutoEnhance(p.auto_enhance);
        if (ENHANCE_MODELS.some((m) => m.name === p.enhance_model)) setEnhanceModel(p.enhance_model);
      })
      .catch(() => {});
  }, []);

  const startEnhanceDownload = async (modelName: string) => {
    await invoke('whisper_init');
    const models = await invoke<{ name: string; status: unknown }[]>('whisper_get_available_models');
    const have = models.some((m) => m.name === modelName && m.status === 'Available');
    if (!have) {
      invoke('whisper_download_model', { modelName }).catch((e) =>
        console.error('[TranscriptionStep] Enhance model download failed:', e)
      );
    }
  };

  const [parakeetState, setParakeetState] = useState<DownloadState>({
    status: parakeetDownloaded ? 'completed' : parakeetError ? 'error' : 'waiting',
    error: parakeetError ?? undefined,
    progress: parakeetDownloaded ? 100 : 0,
    downloadedMb: 0,
    totalMb: 670,
    speedMbps: 0,
  });

  const parakeetDownloadStartedRef = useRef(false);
  const retryingRef = useRef(false);

  // Retry download handler
  const handleRetryDownload = async () => {
    // Prevent multiple simultaneous retries
    if (retryingRef.current) {
      console.log('[DownloadProgressStep] Retry already in progress, ignoring');
      return;
    }

    console.log('[DownloadProgressStep] Retrying Parakeet download');
    retryingRef.current = true;

    // Reset error state
    setParakeetState((prev) => ({
      ...prev,
      status: 'waiting',
      error: undefined,
      progress: 0,
      downloadedMb: 0,
      speedMbps: 0,
    }));

    try {
      await invoke('parakeet_retry_download', { modelName: PARAKEET_MODEL });
      // Progress events will update state
    } catch (error) {
      console.error('[DownloadProgressStep] Retry failed:', error);
      setParakeetState((prev) => ({
        ...prev,
        status: 'error',
        error: error instanceof Error ? error.message : 'Retry failed',
      }));

      toast.error('Download retry failed', {
        description: 'Please check your connection and try again.',
      });
    } finally {
      // Allow retry again after 2 seconds
      setTimeout(() => {
        retryingRef.current = false;
      }, 2000);
    }
  };

  // Start the required transcription model immediately.
  useEffect(() => {
    if (parakeetDownloadStartedRef.current) return;
    parakeetDownloadStartedRef.current = true;

    if (!parakeetDownloaded && !parakeetError) {
      setParakeetState((prev) => ({ ...prev, status: 'downloading' }));
    }

    startBackgroundDownloads({
      includeParakeet: true,
    }).catch((error) => {
      console.error('Failed to start Parakeet download:', error);
      if (!parakeetDownloaded) {
        setParakeetState((prev) => ({ ...prev, status: 'error', error: String(error) }));
      }
    });
  }, []);

  // Listen to Parakeet download progress
  useEffect(() => {
    const unlistenProgress = listen<ParakeetDownloadProgressEvent>(
      'parakeet-model-download-progress',
      (event) => {
        const { modelName, progress, downloaded_mb, total_mb, speed_mbps, status } = event.payload;
        if (modelName !== PARAKEET_MODEL) return;

        if (status === 'cancelled') {
          setParakeetState((prev) => ({
            ...prev,
            status: 'cancelled',
            progress: 0,
            downloadedMb: 0,
            speedMbps: 0,
          }));
          setParakeetDownloaded(false);
          return;
        }

        setParakeetState((prev) => ({
          ...prev,
          status: status === 'completed' ? 'completed' : 'downloading',
          progress,
          downloadedMb: downloaded_mb ?? prev.downloadedMb,
          totalMb: total_mb ?? prev.totalMb,
          speedMbps: speed_mbps ?? prev.speedMbps,
        }));

        if (status === 'completed') {
          setParakeetDownloaded(true);
        }
      }
    );

    const unlistenComplete = listen<{ modelName: string }>(
      'parakeet-model-download-complete',
      (event) => {
        if (event.payload.modelName === PARAKEET_MODEL) {
          setParakeetState((prev) => ({ ...prev, status: 'completed', progress: 100 }));
          setParakeetDownloaded(true);
        }
      }
    );

    const unlistenError = listen<{ modelName: string; error: string }>(
      'parakeet-model-download-error',
      (event) => {
        if (event.payload.modelName === PARAKEET_MODEL) {
          setParakeetState((prev) => ({
            ...prev,
            status: 'error',
            error: event.payload.error,
          }));
        }
      }
    );

    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenComplete.then((fn) => fn());
      unlistenError.then((fn) => fn());
    };
  }, []);

  const handleContinue = async () => {
    if (continuing) return;
    setContinuing(true);
    try {
      await continueFlow();
    } finally {
      setContinuing(false);
    }
  };

  const continueFlow = async () => {
    // Verify actual model availability (catches state drift)
    try {
      await invoke('parakeet_init');
      const actuallyAvailable = await invoke<boolean>('parakeet_has_available_models');

      if (actuallyAvailable && !parakeetDownloaded) {
        console.log('[DownloadProgressStep] Model available but state not updated');
        setParakeetDownloaded(true);
        setParakeetState((prev) => ({
          ...prev,
          status: 'completed',
          progress: 100,
        }));
      } else if (
        !actuallyAvailable &&
        (parakeetState.status === 'error' || parakeetState.status === 'cancelled')
      ) {
        toast.error('Transcription engine required', {
          description: 'Please retry the download before continuing.',
        });
        return;
      }
    } catch (error) {
      console.warn('[DownloadProgressStep] Failed to verify model:', error);
    }

    setSaveError('');
    try {
      await savePreferences(
        autoEnhance
          ? { auto_enhance: true, enhance_provider: 'whisper', enhance_model: enhanceModel }
          : { auto_enhance: false }
      );
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : String(e));
      return;
    }
    if (autoEnhance) {
      startEnhanceDownload(enhanceModel).catch((e) => console.error('[TranscriptionStep] ', e));
    }
    goToStep(isMac ? 4 : 5);
  };

  const renderDownloadCard = (
    title: string,
    icon: React.ReactNode,
    state: DownloadState,
    modelSize: string,
    sizeUnit = 'MB'
  ) => (
    <div className="bg-background rounded-lg border border-border p-5">
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-full bg-muted flex items-center justify-center">
            {icon}
          </div>
          <div>
            <h3 className="font-medium text-foreground">{title}</h3>
            <p className="text-sm text-muted-foreground">{modelSize}</p>
          </div>
        </div>
        <div>
          {state.status === 'waiting' && (
            <span className="text-sm text-muted-foreground">Waiting...</span>
          )}
          {state.status === 'downloading' && (
            <Loader2 className="w-5 h-5 text-foreground animate-spin" />
          )}
          {state.status === 'completed' && (
            <div className="w-6 h-6 rounded-full bg-success/10 flex items-center justify-center">
              <Check className="w-4 h-4 text-success" />
            </div>
          )}
          {state.status === 'error' && (
            <span className="text-sm text-destructive">Failed</span>
          )}
          {state.status === 'cancelled' && (
            <span className="text-sm text-muted-foreground">Cancelled</span>
          )}
        </div>
      </div>

      {/* Progress Bar */}
      {(state.status === 'downloading' || state.status === 'completed') && (
        <div className="space-y-2">
          <div className="w-full h-2 bg-accent rounded-full overflow-hidden">
            <div
              className="h-full bg-primary rounded-full transition-all duration-300"
              style={{ width: `${state.progress}%` }}
            />
          </div>
          <div className="flex items-center justify-between text-sm">
            <span className="text-muted-foreground">
              {state.downloadedMb.toFixed(1)} {sizeUnit} / {state.totalMb.toFixed(1)} {sizeUnit}
            </span>
            <div className="flex items-center gap-2">
              {state.speedMbps > 0 && (
                <span className="text-muted-foreground">
                  {state.speedMbps.toFixed(1)} {sizeUnit}/s
                </span>
              )}
              <span className="font-semibold text-foreground">
                {Math.round(state.progress)}%
              </span>
            </div>
          </div>
        </div>
      )}

      {(state.status === 'error' || state.status === 'cancelled') && (
        <div className="mt-2 p-3 bg-destructive/10 border border-destructive/40 rounded-md">
          <p className="text-sm text-destructive font-medium">
            {state.status === 'cancelled' ? 'Download cancelled' : 'Download Error'}
          </p>
          {state.error && <p className="text-xs text-destructive mt-1">{state.error}</p>}
          {title === 'Transcription Engine' && (
            <button
              onClick={handleRetryDownload}
              className="mt-3 w-full h-9 px-4 bg-primary hover:bg-primary/90 text-primary-foreground text-sm font-medium rounded-md transition-colors flex items-center justify-center gap-2"
            >
              <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2}
                      d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
              Try Again
            </button>
          )}
        </div>
      )}
    </div>
  );

  return (
    <OnboardingContainer
      title="Transcription"
      description="A live transcript while you talk, and a more accurate one after the call."
      step={3}
      showNavigation
      canGoNext={false}
    >
      <div className="flex flex-col items-center space-y-6">
        <div className="w-full max-w-lg space-y-4">
          <p className="text-sm font-medium">Live transcript</p>
          {renderDownloadCard(
            'Transcription Engine',
            <Mic className="w-5 h-5 text-muted-foreground" />,
            parakeetState,
            '~670 MB'
          )}

          <div className="bg-background rounded-lg border border-border p-5 space-y-4">
            <div className="flex items-center justify-between gap-4">
              <div>
                <h3 className="font-medium text-foreground">Enhance after each call</h3>
                <p className="text-sm text-muted-foreground">Re-transcribes the full recording with Whisper.</p>
              </div>
              <Switch checked={autoEnhance} onCheckedChange={setAutoEnhance} aria-label="Enhance after each call" />
            </div>
            {autoEnhance && (
              <div className="grid grid-cols-3 gap-2" role="radiogroup" aria-label="Enhance model">
                {ENHANCE_MODELS.map((m) => {
                  const on = enhanceModel === m.name;
                  return (
                    <button key={m.name} type="button" role="radio" aria-checked={on}
                      onClick={() => setEnhanceModel(m.name)}
                      className={`rounded-md border p-3 text-left ${on ? 'border-foreground' : 'border-border hover:border-foreground/50'}`}>
                      <p className="text-sm font-medium text-foreground">{m.label}</p>
                      <p className="text-xs text-muted-foreground">{m.size} · {m.hint}</p>
                    </button>
                  );
                })}
              </div>
            )}
            {autoEnhance && <p className="text-xs text-muted-foreground">It keeps downloading after setup.</p>}
          </div>
        </div>

        <div className="w-full max-w-xs space-y-2">
          <Button
            onClick={handleContinue}
            disabled={continuing}
            className="w-full h-11 bg-primary hover:bg-primary/90 text-primary-foreground"
          >
            Continue
          </Button>
          {!parakeetDownloaded && (
            <p className="text-xs text-center text-muted-foreground">The live engine keeps downloading. You can continue.</p>
          )}
          {saveError && <p className="text-xs text-center text-destructive">{saveError}</p>}
        </div>
      </div>
    </OnboardingContainer>
  );
}
