#!/usr/bin/env bash
set -euo pipefail

: "${SMESH_TOKEN:?Set SMESH_TOKEN to a ScientiaMesh access token.}"
: "${SMESH_MESH_ID:?Set SMESH_MESH_ID to the target mesh id.}"

note="${1:-}"
if [ -z "$note" ]; then
  printf 'usage: %s "note to capture"\n' "$0" >&2
  exit 2
fi

export SMESH_AGENT_MODE=1

smesh capture text "$note" \
  --instructions "Capture this OpenClaw/Codex agent update as a work log." \
  --tag openclaw \
  --tag codex \
  --tag agent-workflow
