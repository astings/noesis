'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { SettingsGroup, SettingRow } from '@/components/SettingsGroup';

/** Generic settings section 5: register the published MCP server with Claude Code. */
export function McpServerSettings({ disabled }: { disabled?: boolean }) {
  const [command, setCommand] = useState('');
  const [registering, setRegistering] = useState(false);

  useEffect(() => {
    invoke<string>('get_mcp_register_command').then(setCommand).catch(() => setCommand(''));
  }, []);

  const register = async () => {
    setRegistering(true);
    try {
      const message = await invoke<string>('register_mcp_with_claude');
      toast.success('MCP server registered', { description: message });
    } catch (error) {
      toast.error('Could not register the MCP server', { description: error instanceof Error ? error.message : String(error) });
    } finally {
      setRegistering(false);
    }
  };
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      toast.success('Command copied');
    } catch {
      toast.error('Could not copy', { description: command });
    }
  };

  return (
    <SettingsGroup title="Claude Code">
      <SettingRow
        label="MCP server"
        hint={
          <>
            Lets Claude Code read your meetings from any project. Needs Node 22.18 or newer.
            <span className="block mt-1 font-mono break-all">{command}</span>
          </>
        }
        control={
          <div className="flex gap-1">
            <Button type="button" variant="outline" size="sm" onClick={register} disabled={disabled || registering}>
              {registering ? 'Registering…' : 'Register with Claude Code'}
            </Button>
            <Button type="button" variant="ghost" size="sm" onClick={copy} disabled={!command}>
              Copy command
            </Button>
          </div>
        }
      />
    </SettingsGroup>
  );
}
