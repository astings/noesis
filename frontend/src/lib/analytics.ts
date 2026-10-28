// Analytics is disabled in this fork. The class keeps its signatures so call
// sites compile; every method is a no-op.

export interface AnalyticsProperties {
  [key: string]: string;
}

export interface DeviceInfo {
  platform: string;
  os_version: string;
  architecture: string;
}

export interface UserSession {
  session_id: string;
  user_id: string;
  start_time: string;
  last_heartbeat: string;
  is_active: boolean;
}

export class Analytics {
  static async init(): Promise<void> {}
  static async disable(): Promise<void> {}
  static async isEnabled(): Promise<boolean> { return false; }
  static async track(eventName: string, properties?: AnalyticsProperties): Promise<void> {}
  static async identify(userId: string, properties?: AnalyticsProperties): Promise<void> {}
  static async startSession(userId: string): Promise<string | null> { return null; }
  static async endSession(): Promise<void> {}
  static async trackDailyActiveUser(): Promise<void> {}
  static async trackUserFirstLaunch(): Promise<void> {}
  static async isSessionActive(): Promise<boolean> { return false; }
  static async getPersistentUserId(): Promise<string> { return ''; }
  static async checkAndTrackFirstLaunch(): Promise<void> {}
  static async checkAndTrackDailyUsage(): Promise<void> {}
  static getCurrentUserId(): string | null { return null; }
  static async getPlatform(): Promise<string> { return 'unknown'; }
  static async getOSVersion(): Promise<string> { return 'unknown'; }
  static async getDeviceInfo(): Promise<DeviceInfo> {
    return { platform: 'unknown', os_version: 'unknown', architecture: 'unknown' };
  }
  static async calculateDaysSince(dateKey: string): Promise<number | null> { return null; }
  static async updateMeetingCount(): Promise<void> {}
  static async getMeetingsCountToday(): Promise<number> { return 0; }
  static async hasUsedFeatureBefore(featureName: string): Promise<boolean> { return false; }
  static async markFeatureUsed(featureName: string): Promise<void> {}
  static async trackSessionStarted(sessionId: string): Promise<void> {}
  static async trackSessionEnded(sessionId: string): Promise<void> {}
  static async trackMeetingCompleted(meetingId: string, metrics: {
    duration_seconds: number;
    transcript_segments: number;
    transcript_word_count: number;
    words_per_minute: number;
    meetings_today: number;
  }): Promise<void> {}
  static async trackFeatureUsedEnhanced(featureName: string, properties?: Record<string, any>): Promise<void> {}
  static async trackCopy(copyType: 'transcript' | 'summary', properties?: Record<string, any>): Promise<void> {}
  static async trackMeetingStarted(meetingId: string): Promise<void> {}
  static async trackRecordingStarted(meetingId: string): Promise<void> {}
  static async trackRecordingStopped(meetingId: string, durationSeconds?: number): Promise<void> {}
  static async trackMeetingDeleted(meetingId: string): Promise<void> {}
  static async trackSettingsChanged(settingType: string, newValue: string): Promise<void> {}
  static async trackFeatureUsed(featureName: string): Promise<void> {}
  static async trackPageView(pageName: string): Promise<void> {}
  static async trackButtonClick(buttonName: string, location?: string): Promise<void> {}
  static async trackError(errorType: string, errorMessage: string): Promise<void> {}
  static async trackAppStarted(): Promise<void> {}
  static async cleanup(): Promise<void> {}
  static reset(): void {}
  static async waitForInitialization(timeout: number = 5000): Promise<boolean> { return false; }
  static async trackBackendConnection(success: boolean, error?: string): Promise<void> {}
  static async trackTranscriptionError(errorMessage: string): Promise<void> {}
  static async trackTranscriptionSuccess(duration?: number): Promise<void> {}
  static async trackSummaryGenerationStarted(
    modelProvider: string,
    modelName: string,
    transcriptLength: number,
    timeSinceRecordingMinutes?: number
  ): Promise<void> {}
  static async trackSummaryGenerationCompleted(
    modelProvider: string,
    modelName: string,
    success: boolean,
    durationSeconds?: number,
    errorMessage?: string
  ): Promise<void> {}
  static async trackSummaryRegenerated(modelProvider: string, modelName: string): Promise<void> {}
  static async trackModelChanged(oldProvider: string, oldModel: string, newProvider: string, newModel: string): Promise<void> {}
  static async trackCustomPromptUsed(promptLength: number): Promise<void> {}
}
export default Analytics;
