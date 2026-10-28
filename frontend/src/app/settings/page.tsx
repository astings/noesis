'use client';

import React, { useState, useEffect } from 'react';
import { ArrowLeft } from 'lucide-react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';
import { TranscriptSettings } from '@/components/TranscriptSettings';
import { RecordingSettings } from '@/components/RecordingSettings';
import { PreferenceSettings } from '@/components/PreferenceSettings';
import { useConfig } from '@/contexts/ConfigContext';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';

// Tabs configuration (constant)
const TABS = [
  { value: 'general', label: 'General' },
  { value: 'recording', label: 'Recordings' },
  { value: 'Transcriptionmodels', label: 'Transcription' },
] as const;

export default function SettingsPage() {
  const router = useRouter();
  const { transcriptModelConfig, setTranscriptModelConfig } = useConfig();

  const [activeTab, setActiveTab] = useState('general');

  // Load saved transcript configuration on mount
  useEffect(() => {
    const loadTranscriptConfig = async () => {
      try {
        const config = await invoke('api_get_transcript_config') as any;
        if (config) {
          console.log('Loaded saved transcript config:', config);
          setTranscriptModelConfig({
            provider: config.provider || 'localWhisper',
            model: config.model || 'large-v3',
            apiKey: config.apiKey || null
          });
        }
      } catch (error) {
        console.error('Failed to load transcript config:', error);
      }
    };
    loadTranscriptConfig();
  }, [setTranscriptModelConfig]);

  return (
    <div className="h-screen bg-background flex flex-col">
      <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border">
        <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => router.back()} aria-label="Back">
          <ArrowLeft />
        </Button>
        <h1 className="text-lg font-semibold">Settings</h1>
      </div>

      <Tabs value={activeTab} onValueChange={setActiveTab} className="flex-1 min-h-0 flex flex-col">
        <TabsList className="h-auto w-full justify-start gap-4 rounded-none border-b border-border bg-transparent p-0 px-6">
          {TABS.map((tab) => (
            <TabsTrigger
              key={tab.value}
              value={tab.value}
              className="rounded-none border-b-2 border-transparent bg-transparent px-0 py-2.5 text-[13px] font-normal text-muted-foreground hover:text-foreground data-[state=active]:border-foreground data-[state=active]:bg-transparent data-[state=active]:text-foreground data-[state=active]:shadow-none"
            >
              {tab.label}
            </TabsTrigger>
          ))}
        </TabsList>

        <div className="flex-1 overflow-y-auto px-6 py-5">
          <TabsContent value="general" className="mt-0">
            <PreferenceSettings />
          </TabsContent>
          <TabsContent value="recording" className="mt-0">
            <RecordingSettings />
          </TabsContent>
          <TabsContent value="Transcriptionmodels" className="mt-0">
            <TranscriptSettings
              transcriptModelConfig={transcriptModelConfig}
              setTranscriptModelConfig={setTranscriptModelConfig}
            />
          </TabsContent>
        </div>
      </Tabs>
    </div>
  );
};
