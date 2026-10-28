#!/bin/sh
# spec 4.7: reference post-enhance command for Noesis (advanced). Most users should use the "Wiki automation"
# table in Settings instead; this script does the same by hand. To use it, set "Custom command (advanced)" to this file's path.
# The app runs it with NOESIS_FOLDER, NOESIS_PROJECT, NOESIS_TITLE and NOESIS_MEETING_ID set,
# from inside the meeting folder, and appends this script's output to hook.log there.
# DRY_RUN=1 prints the claude command instead of running it.
# A successful run writes hook.done in the meeting folder; delete that file to let a re-enhance reprocess the meeting.
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

# Project to wiki directory and skill. Add a line per project.
case "$NOESIS_PROJECT" in
  dex)        WIKI="$HOME/Code/Case/DEX/dex-case-tracking";     SKILL="dex-case-tracking" ;;
  recruiting) WIKI="$HOME/Code/Knowledge/recruiting";           SKILL="process-interview" ;;
  flo)        WIKI="$HOME/Code/Case/FLO/revenue-ai-knowledge";  SKILL="" ;;
  *)          exit 0 ;;   # _inbox and unknown projects: nothing to do
esac

MARKER="$NOESIS_FOLDER/hook.done"
if [ -e "$MARKER" ]; then
  echo "already processed ($MARKER exists), skipping"
  exit 0
fi
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
  || { rc=$?; echo "claude failed with exit $rc, no hook.done written" >&2; exit "$rc"; }
date > "$MARKER"
echo "done"
