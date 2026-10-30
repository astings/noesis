import React, { useState } from 'react';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { McpServerSettings } from '@/components/McpServerSettings';

export function ClaudeCodeStep() {
  const { completeOnboarding, parakeetDownloaded, parakeetProgress } = useOnboarding();
  const [finishing, setFinishing] = useState(false);
  const [error, setError] = useState('');

  const handleFinish = async () => {
    setFinishing(true);
    setError('');
    try {
      await completeOnboarding();
      window.location.reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setFinishing(false);
    }
  };

  return (
    <OnboardingContainer
      title="Claude Code"
      description="Optional: let Claude Code read your meetings."
      step={6}
      showNavigation
      canGoNext={false}
    >
      <div className="max-w-md mx-auto space-y-6">
        <McpServerSettings />
        <p className="text-xs text-muted-foreground">
          No Claude Code? Install it from https://claude.com/claude-code, then use Settings &gt; Recordings &gt; Claude Code.
        </p>
        <div className="space-y-2">
          <Button onClick={handleFinish} disabled={!parakeetDownloaded || finishing} className="w-full h-11">
            {finishing ? <Loader2 className="w-4 h-4 animate-spin" /> : 'Finish'}
          </Button>
          {!parakeetDownloaded && (
            <p className="text-xs text-center text-muted-foreground">
              Downloading the live transcription engine… {Math.round(parakeetProgress)}%
            </p>
          )}
          {error && <p className="text-xs text-center text-destructive">{error}</p>}
        </div>
      </div>
    </OnboardingContainer>
  );
}
