//! Generic settings section 5: the published MCP server and its Claude Code registration.

use std::path::PathBuf;
use std::process::Command;

pub const MCP_PACKAGE: &str = "@noesis-app/noesis-mcp";
pub const MCP_REGISTER_ARGS: [&str; 9] = ["mcp", "add", "--scope", "user", "noesis", "--", "npx", "-y", MCP_PACKAGE];

/// The same registration as a shell line, for Codex and Cursor users to adapt.
pub fn mcp_register_command() -> String {
    format!("claude {}", MCP_REGISTER_ARGS.join(" "))
}

/// The app launched from Finder has a minimal PATH; the user bins come first.
fn path_env() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let inherited = std::env::var("PATH").unwrap_or_default();
    format!("{}/.local/bin:/opt/homebrew/bin:/usr/local/bin:{}", home, inherited)
}

fn locate_claude() -> Option<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [
        PathBuf::from(format!("{}/.local/bin/claude", home)),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ];
    candidates.into_iter().find(|p| p.is_file()).or_else(|| which::which("claude").ok())
}

#[tauri::command]
pub fn get_mcp_register_command() -> String {
    mcp_register_command()
}

/// Runs `claude mcp add ...` with the located Claude Code. Returns its output, or the error text.
#[tauri::command]
pub async fn register_mcp_with_claude() -> Result<String, String> {
    let claude = locate_claude().ok_or_else(|| "Claude Code (claude) not found. Install it and sign in, then try again.".to_string())?;
    let output = tokio::task::spawn_blocking(move || Command::new(claude).args(MCP_REGISTER_ARGS).env("PATH", path_env()).output())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Could not start Claude Code: {}", e))?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)).trim().to_string();
    if output.status.success() {
        Ok(if text.is_empty() { "Registered. Restart Claude Code sessions to see the noesis tools.".to_string() } else { text })
    } else {
        Err(if text.is_empty() { format!("claude exited with {}", output.status) } else { text })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_register_command_uses_npx_and_user_scope() {
        assert_eq!(mcp_register_command(), "claude mcp add --scope user noesis -- npx -y @noesis-app/noesis-mcp");
        assert_eq!(MCP_REGISTER_ARGS.len(), 9);
    }

    #[test]
    fn path_env_puts_the_user_bins_first() {
        let p = path_env();
        assert!(p.starts_with(&format!("{}/.local/bin:/opt/homebrew/bin:/usr/local/bin:", std::env::var("HOME").unwrap())), "{}", p);
    }
}
