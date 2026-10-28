'use client';

import type { ReactNode } from 'react';

interface MeetingDetailsSplitViewProps {
  transcript: ReactNode;
}

export function MeetingDetailsSplitView({ transcript }: MeetingDetailsSplitViewProps) {
  return (
    <div
      role="region"
      aria-label="Transcript"
      className="flex flex-1 min-h-0 min-w-0 flex-col overflow-hidden"
    >
      {transcript}
    </div>
  );
}
