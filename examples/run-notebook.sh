#!/usr/bin/env bash
# Worked example: create a note, run it, poll until it finishes, print the output, clean up.
#
#   ./examples/run-notebook.sh [notebook.json] [--profile NAME]
#
# Needs: zeppelin (this CLI) and jq on PATH, plus a profile in ~/.zeppelin.toml.
set -euo pipefail

NOTEBOOK=${1:-"$(dirname "$0")/notebooks/hello-shell.json"}
shift || true
Z=(zeppelin "$@")                      # e.g. pass --profile staging
NAME="/tmp/cli-run-$(date +%s)-$$"     # unique path so parallel runs don't clash

# 1. Create the note from the JSON file. The CLI prints just the new note id.
NOTE_ID=$("${Z[@]}" note create --from "$NOTEBOOK" --name "$NAME")
echo "created note $NOTE_ID at $NAME" >&2

# 4. Always clean up, even if a step below fails or the script is interrupted.
cleanup() { "${Z[@]}" note delete "$NOTE_ID" >/dev/null && echo "deleted note $NOTE_ID" >&2; }
trap cleanup EXIT

# 2. Start every paragraph (asynchronous: returns immediately).
"${Z[@]}" note run "$NOTE_ID"

# 3. Poll the job status until the note is no longer running.
deadline=$((SECONDS + 600))
while :; do
  status=$("${Z[@]}" note status "$NOTE_ID" --compact)
  running=$(jq -r '.isRunning' <<<"$status")
  active=$(jq '[.paragraphs[] | select(.status == "PENDING" or .status == "RUNNING")] | length' <<<"$status")
  echo "isRunning=$running active=$active $(jq -r '[.paragraphs[].status] | join(",")' <<<"$status")" >&2
  [[ $running == false && $active == 0 ]] && break
  (( SECONDS < deadline )) || { echo "timed out" >&2; "${Z[@]}" note stop "$NOTE_ID"; exit 4; }
  sleep 2
done

# Report the results.
"${Z[@]}" note output "$NOTE_ID"
failed=$(jq '[.paragraphs[] | select(.status == "ERROR" or .status == "ABORTED")] | length' <<<"$status")
if (( failed > 0 )); then
  jq -r '.paragraphs[] | select(.status == "ERROR" or .status == "ABORTED") | "FAILED \(.id): \(.errorMessage // "")"' <<<"$status" >&2
  exit 3
fi
echo "all paragraphs finished" >&2
