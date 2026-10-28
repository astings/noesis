interface UseMeetingDataProps {
  meeting: any;
}

export function useMeetingData({ meeting }: UseMeetingDataProps) {
  return {
    transcripts: meeting.transcripts,
    meetingTitle: meeting.title || '+ New Call',
  };
}
