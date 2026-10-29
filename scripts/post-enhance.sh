#!/bin/sh
# spec 4.7: reference post-enhance command for Noesis (advanced). Most users should use the "Post-processing" card in Settings instead; this script does the same by hand. To use it, set "Custom command" to this file's path.
# The app runs it with NOESIS_FOLDER, NOESIS_PROJECT, NOESIS_TITLE and NOESIS_MEETING_ID set,
# from inside the meeting folder, and appends this script's output to hook.log there.
# DRY_RUN=1 prints the claude command instead of running it.
# The app owns hook.done (written on exit 0, checked before an automatic run); this script neither reads nor writes it.
# Exit 2 means no wiki is configured for the project, which the app records as a failed hook.
# The claude call may only read and write wiki pages: delete, move and assign_meeting are denied because the
# session is unattended and its input is other people's words.
set -eu

# The app launched from Finder has a minimal PATH; claude lives in ~/.local/bin on this machine.
export PATH="$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

NOESIS_FOLDER="${NOESIS_FOLDER:-${MEETILY_FOLDER:-}}"
NOESIS_PROJECT="${NOESIS_PROJECT:-${MEETILY_PROJECT:-}}"
NOESIS_MEETING_ID="${NOESIS_MEETING_ID:-${MEETILY_MEETING_ID:-}}"
: "${NOESIS_FOLDER:?NOESIS_FOLDER is required}"
: "${NOESIS_PROJECT:?NOESIS_PROJECT is required}"
: "${NOESIS_MEETING_ID:?NOESIS_MEETING_ID is required}"
NOESIS_TITLE="${NOESIS_TITLE:-${MEETILY_TITLE:-}}"

# Project to directory and optional instructions. One line per project; these two are examples.
case "$NOESIS_PROJECT" in
  example)   WIKI="$HOME/path/to/example-notes";   SKILL="process-meeting" ;;
  another)   WIKI="$HOME/path/to/another-notes";   SKILL="" ;;
  *)         echo "no directory configured for project $NOESIS_PROJECT"; exit 2 ;;
esac

if [ ! -d "$WIKI" ]; then
  echo "wiki directory $WIKI not found" >&2
  exit 1
fi

if [ -n "$SKILL" ]; then
  PROMPT="Use the $SKILL skill to process the new meeting recording at $NOESIS_FOLDER (project $NOESIS_PROJECT, meeting id $NOESIS_MEETING_ID, title: $NOESIS_TITLE). Draft only: write the transcript document and a draft log entry; do not score, do not update trackers or the Blueprint. Leave everything uncommitted for review. This run is unattended: do not ask questions; if the wiki already covers this meeting, stop without changes."
else
  PROMPT="Process the new meeting recording at $NOESIS_FOLDER (project $NOESIS_PROJECT, meeting id $NOESIS_MEETING_ID, title: $NOESIS_TITLE). Draft only: write the transcript document and a draft log entry; leave everything uncommitted for review. This run is unattended: do not ask questions; if the wiki already covers this meeting, stop without changes."
fi

cd "$WIKI"
# Cap a runaway headless session; a draft should cost far less than this.
MAX_BUDGET_USD="${MAX_BUDGET_USD:-5}"

if [ "${DRY_RUN:-0}" = "1" ]; then
  printf '%s\n' "DRY_RUN in $WIKI: claude -p \"$PROMPT\" --permission-mode acceptEdits --allowedTools mcp__open-knowledge mcp__noesis --disallowedTools mcp__open-knowledge__delete mcp__open-knowledge__move mcp__noesis__assign_meeting --max-budget-usd $MAX_BUDGET_USD"
  exit 0
fi

echo "processing $NOESIS_MEETING_ID in $WIKI"
claude -p "$PROMPT" --permission-mode acceptEdits --allowedTools mcp__open-knowledge mcp__noesis \
  --disallowedTools mcp__open-knowledge__delete mcp__open-knowledge__move mcp__noesis__assign_meeting \
  --max-budget-usd "$MAX_BUDGET_USD" </dev/null \
  || { rc=$?; echo "claude failed with exit $rc, hook failed" >&2; exit "$rc"; }
echo "done"
