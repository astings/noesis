"use client"

import { useEffect, useState } from "react"
import { Switch } from "./ui/switch"
import { FolderOpen } from "lucide-react"
import { Button } from "./ui/button"
import { SettingsGroup, SettingRow } from "./SettingsGroup"
import { invoke } from "@tauri-apps/api/core"
import { useConfig, NotificationSettings } from "@/contexts/ConfigContext"

export function PreferenceSettings() {
  const {
    notificationSettings,
    storageLocations,
    isLoadingPreferences,
    loadPreferences,
    updateNotificationSettings
  } = useConfig();

  const [notificationsEnabled, setNotificationsEnabled] = useState<boolean | null>(null);
  const [isInitialLoad, setIsInitialLoad] = useState(true);
  const [previousNotificationsEnabled, setPreviousNotificationsEnabled] = useState<boolean | null>(null);

  // Lazy load preferences on mount (only loads if not already cached)
  useEffect(() => {
    loadPreferences();
  }, [loadPreferences]);

  // Update notificationsEnabled when notificationSettings are loaded from global state
  useEffect(() => {
    if (notificationSettings) {
      // Notification enabled means both started and stopped notifications are enabled
      const enabled =
        notificationSettings.notification_preferences.show_recording_started &&
        notificationSettings.notification_preferences.show_recording_stopped;
      setNotificationsEnabled(enabled);
      if (isInitialLoad) {
        setPreviousNotificationsEnabled(enabled);
        setIsInitialLoad(false);
      }
    } else if (!isLoadingPreferences) {
      // If not loading and no settings, use default
      setNotificationsEnabled(true);
      if (isInitialLoad) {
        setPreviousNotificationsEnabled(true);
        setIsInitialLoad(false);
      }
    }
  }, [notificationSettings, isLoadingPreferences, isInitialLoad])

  useEffect(() => {
    // Skip update on initial load or if value hasn't actually changed
    if (isInitialLoad || notificationsEnabled === null || notificationsEnabled === previousNotificationsEnabled) return;
    if (!notificationSettings) return;

    const handleUpdateNotificationSettings = async () => {
      console.log("Updating notification settings to:", notificationsEnabled);

      try {
        // Update the notification preferences
        const updatedSettings: NotificationSettings = {
          ...notificationSettings,
          notification_preferences: {
            ...notificationSettings.notification_preferences,
            show_recording_started: notificationsEnabled,
            show_recording_stopped: notificationsEnabled,
          }
        };

        console.log("Calling updateNotificationSettings with:", updatedSettings);
        await updateNotificationSettings(updatedSettings);
        setPreviousNotificationsEnabled(notificationsEnabled);
        console.log("Successfully updated notification settings to:", notificationsEnabled);

      } catch (error) {
        console.error('Failed to update notification settings:', error);
      }
    };

    handleUpdateNotificationSettings();
  }, [notificationsEnabled, notificationSettings, isInitialLoad, previousNotificationsEnabled, updateNotificationSettings])

  const handleOpenFolder = async (folderType: 'database' | 'models' | 'recordings') => {
    try {
      switch (folderType) {
        case 'database':
          await invoke('open_database_folder');
          break;
        case 'models':
          await invoke('open_models_folder');
          break;
        case 'recordings':
          await invoke('open_recordings_folder');
          break;
      }

    } catch (error) {
      console.error(`Failed to open ${folderType} folder:`, error);
    }
  };

  // Show loading only if we're actually loading and don't have cached data
  if (isLoadingPreferences && !notificationSettings && !storageLocations) {
    return <div className="text-sm text-muted-foreground">Loading preferences...</div>
  }

  // Show loading if notificationsEnabled hasn't been determined yet
  if (notificationsEnabled === null && !isLoadingPreferences) {
    return <div className="text-sm text-muted-foreground">Loading preferences...</div>
  }

  // Ensure we have a boolean value for the Switch component
  const notificationsEnabledValue = notificationsEnabled ?? false;

  return (
    <div className="space-y-6">
      <SettingsGroup>
        <SettingRow
          label="Notifications"
          hint="Notify when a meeting recording starts and ends"
          control={<Switch checked={notificationsEnabledValue} onCheckedChange={setNotificationsEnabled} />}
        />
      </SettingsGroup>

      <SettingsGroup title="Data storage">
        <SettingRow
          label="Meeting recordings"
          hint={<span className="font-mono break-all">{storageLocations?.recordings || 'Loading...'}</span>}
          control={
            <Button variant="outline" size="sm" onClick={() => handleOpenFolder('recordings')}>
              <FolderOpen />
              Open folder
            </Button>
          }
        />
        <SettingRow
          label="Database and models"
          hint="Stored together in the application data directory"
        />
      </SettingsGroup>
    </div>
  )
}
