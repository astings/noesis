// spec 4.2: project helpers shared by the record button and meeting details.
export const INBOX = '_inbox';
const LAST_PROJECT_KEY = 'meetily.lastProject';

/** The project chosen last time. Per-device convenience, so localStorage is fine. */
export function getLastProject(): string {
  try {
    return localStorage.getItem(LAST_PROJECT_KEY) || INBOX;
  } catch {
    return INBOX;
  }
}

export function setLastProject(project: string): void {
  try {
    localStorage.setItem(LAST_PROJECT_KEY, project);
  } catch {
    // storage unavailable: the picker still works for this session
  }
}

/** Parent directory name when the folder sits two levels under the root, `_inbox` otherwise. */
export function projectOfFolder(root: string, folderPath: string): string {
  const normalizedRoot = root.replace(/\/+$/, '');
  if (!folderPath.startsWith(normalizedRoot + '/')) return INBOX;
  const parts = folderPath.slice(normalizedRoot.length + 1).split('/').filter(Boolean);
  return parts.length >= 2 ? parts[0] : INBOX;
}

/** Lowercase, dashes, no leading dot: the same shape the Rust side accepts. */
export function normalizeProjectName(input: string): string {
  return input.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 40);
}
