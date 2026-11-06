'use client'

import './globals.css'
import Sidebar from '@/components/Sidebar'
import { SidebarProvider } from '@/components/Sidebar/SidebarProvider'
import MainContent from '@/components/MainContent'
import { Toaster, toast } from 'sonner'
import "sonner/dist/styles.css"
import { useState, useEffect } from 'react'
import { usePathname } from 'next/navigation'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { TooltipProvider } from '@/components/ui/tooltip'
import { TitleBar } from '@/components/TitleBar'
import { RecordingStateProvider } from '@/contexts/RecordingStateContext'
import { TranscriptProvider } from '@/contexts/TranscriptContext'
import { ConfigProvider } from '@/contexts/ConfigContext'
import { OnboardingProvider } from '@/contexts/OnboardingContext'
import { OnboardingFlow } from '@/components/onboarding'
import { DownloadProgressToastProvider } from '@/components/shared/DownloadProgressToast'
import { RecordingPostProcessingProvider } from '@/contexts/RecordingPostProcessingProvider'
import { AutoStartListener } from '@/components/AutoStartListener'


// export { metadata } from './metadata'

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  const pathname = usePathname()
  const [showOnboarding, setShowOnboarding] = useState(false)
  const [onboardingCompleted, setOnboardingCompleted] = useState(false)

  useEffect(() => {
    // Check onboarding status first
    invoke<{ completed: boolean } | null>('get_onboarding_status')
      .then((status) => {
        const isComplete = status?.completed ?? false
        setOnboardingCompleted(isComplete)

        if (!isComplete) {
          console.log('[Layout] Onboarding not completed, showing onboarding flow')
          setShowOnboarding(true)
        } else {
          console.log('[Layout] Onboarding completed, showing main app')
        }
      })
      .catch((error) => {
        console.error('[Layout] Failed to check onboarding status:', error)
        // Default to showing onboarding if we can't check
        setShowOnboarding(true)
        setOnboardingCompleted(false)
      })
  }, [])

  // Disable context menu in production
  useEffect(() => {
    if (process.env.NODE_ENV === 'production') {
      const handleContextMenu = (e: MouseEvent) => e.preventDefault();
      document.addEventListener('contextmenu', handleContextMenu);
      return () => document.removeEventListener('contextmenu', handleContextMenu);
    }
  }, []);
  useEffect(() => {
    // Listen for tray recording toggle request
    const unlisten = listen('request-recording-toggle', () => {
      console.log('[Layout] Received request-recording-toggle from tray');

      if (showOnboarding) {
        toast.error("Please complete setup first", {
          description: "You need to finish onboarding before you can start recording."
        });
      } else {
        // If in main app, forward to useRecordingStart via window event
        console.log('[Layout] Forwarding to start-recording-from-sidebar');
        window.dispatchEvent(new CustomEvent('start-recording-from-sidebar'));
      }
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, [showOnboarding]);

  if (pathname?.startsWith('/overlay')) {
    return (
      <html lang="en" style={{ background: 'transparent' }}>
        <body className="font-sans" style={{ background: 'transparent' }}>
          <TooltipProvider delayDuration={150}>{children}</TooltipProvider>
        </body>
      </html>
    )
  }

  return (
    <html lang="en">
      <body className="font-sans bg-card">
          <RecordingStateProvider>
            <TranscriptProvider>
              <ConfigProvider>
                <OnboardingProvider>
                    <SidebarProvider>
                      <TooltipProvider delayDuration={150}>
                        <RecordingPostProcessingProvider>
                            {/* Download progress toast provider - listens for background downloads */}
                            <DownloadProgressToastProvider />
                            {/* Call detector auto-start, after the previous save has settled */}
                            <AutoStartListener />

                            <TitleBar />
                            {/* Show onboarding or main app */}
                            {showOnboarding ? (
                              <OnboardingFlow />
                            ) : (
                              <div className="flex">
                                <Sidebar />
                                <MainContent>{children}</MainContent>
                              </div>
                            )}
                        </RecordingPostProcessingProvider>
                      </TooltipProvider>
                    </SidebarProvider>
                </OnboardingProvider>
              </ConfigProvider>
            </TranscriptProvider>
          </RecordingStateProvider>

        <Toaster theme="system" position="bottom-center" richColors closeButton />
      </body>
    </html>
  )
}
