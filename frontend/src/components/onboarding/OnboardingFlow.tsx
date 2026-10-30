import React from 'react';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { WelcomeStep, YouStep, DownloadProgressStep, PermissionsStep } from './steps';

// 1 Welcome, 2 You, 3 Transcription, 4 Permissions (macOS only), 5 Calls, 6 Claude Code.
export function OnboardingFlow() {
  const { currentStep, isMac } = useOnboarding();

  return (
    <div className="onboarding-flow">
      {currentStep === 1 && <WelcomeStep />}
      {currentStep === 2 && <YouStep />}
      {currentStep === 3 && <DownloadProgressStep />}
      {currentStep === 4 && isMac && <PermissionsStep />}
    </div>
  );
}
