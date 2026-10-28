import React, { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';

export function SetupOverviewStep() {
  const { goNext } = useOnboarding();
  const [isMac, setIsMac] = useState(false);

  useEffect(() => {
    const checkPlatform = async () => {
      try {
        const { platform } = await import('@tauri-apps/plugin-os');
        setIsMac(platform() === 'macos');
      } catch (e) {
        setIsMac(navigator.userAgent.includes('Mac'));
      }
    };
    checkPlatform();
  }, []);

  const handleContinue = () => {
    goNext();
  };

  return (
    <OnboardingContainer
      title="Setup Overview"
      description="Meetily requires that you download the transcription model for the software to work."
      step={2}
      totalSteps={isMac ? 4 : 3}
    >
      <div className="flex flex-col items-center space-y-10">
        <div className="w-full max-w-md bg-background rounded-lg border border-border p-4">
          <h3 className="font-medium text-foreground">Download Transcription Engine</h3>
        </div>

        {/* CTA Section */}
        <div className="w-full max-w-xs space-y-4">
          <Button
            onClick={handleContinue}
            className="w-full h-11 bg-primary hover:bg-primary/90 text-primary-foreground"
          >
            Let's Go
          </Button>
          <div className="text-center">
            <a
              href="https://github.com/Zackriya-Solutions/meeting-minutes"
              target="_blank"
              rel="noopener noreferrer"
              className="text-xs text-muted-foreground hover:underline"
            >
              Report issues on GitHub
            </a>
          </div>
        </div>
      </div>
    </OnboardingContainer>
  );
}
