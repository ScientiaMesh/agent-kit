# Agent Usage Guide

The Rust `smesh` binary is safe to call from OpenClaw, Codex, CI jobs, and
other non-interactive agents. Agent mode is enabled when either condition is
true:

- stdout is not a TTY, such as `subprocess.run(..., capture_output=True)`.
- `SMESH_AGENT_MODE=1` is set. `true`, `yes`, and `on` are also accepted.

In agent mode, commands default to JSON output. Interactive terminals still
default to human output. Explicit output flags always win:

```bash
smesh --output human version
smesh --output json auth status
smesh --output ndjson capture status "$CAPTURE_ID"
smesh --json search --mesh-id "$SMESH_MESH_ID" "project memory"
```

## Authentication For Agents

Agents and CI should continue to use non-interactive token auth. Browser login
is available for humans with `smesh auth login`, but it opens a system browser
and waits for a localhost callback, so it is not appropriate for unattended
automation.

Recommended agent auth:

```bash
export SMESH_AGENT_MODE=1
export SMESH_TOKEN="<access-token>"
export SMESH_MESH_ID="<mesh-id>"
smesh --json auth status
```

To persist a profile from an automation-provided token, use:

```bash
smesh --json auth login --access-token "$SMESH_TOKEN" --mesh-id "$SMESH_MESH_ID"
```

For remote human sessions, `smesh auth login --no-open-browser` prints the
Auth0 URL to stderr and waits for the loopback callback. Machine stdout remains
a single JSON document or NDJSON event and does not include bearer or refresh
tokens. The browser must be able to reach the printed `127.0.0.1:<port>` on the
same machine running `smesh`; otherwise, use token auth for that session.

## Stream Contract

- stdout is the only machine data stream.
- JSON mode prints one JSON document followed by `\n`.
- NDJSON mode prints one JSON event per line and includes a `type` field.
- stderr is reserved for non-data diagnostics. In agent mode, command and
  argument errors are rendered as structured JSON on stdout.
- Agent mode does not emit spinners, progress bars, color codes, or prompts.

## Exit Codes

Agents should branch on exit status first, then parse stdout for details.

| Code | Meaning |
| --- | --- |
| `0` | Success. |
| `1` | General, network, serialization, or non-404 HTTP failure. |
| `2` | CLI usage or local configuration error. |
| `4` | Authentication is required or rejected. |
| `5` | Requested remote resource was not found. |
| `6` | Command surface exists but is not supported yet. |

Machine errors use this shape:

```json
{"error":"Missing mesh context. Pass --mesh-id, set SMESH_MESH_ID, or store mesh_id in the selected profile.","status":null,"details":null}
```

## Write Response Contract

Every write command returns an `operation_id`.

- Capture writes reuse the backend job or task id when present, so agents can
  log one id and then poll with `smesh jobs get`.
- Local auth writes generate an `auth-login-*` or `auth-logout-*` id.
- Capture enqueue responses also include `job_id`, `capture_id`, `status`,
  `file_id`/`file_ids`, `source_links`, `links`, and `details` when available.

Example capture response:

```json
{
  "operation_id": "job-123",
  "job_id": "job-123",
  "capture_id": "capture-123",
  "status": "queued",
  "links": {
    "job": "/v1/jobs/job-123",
    "capture": "/v1/captures/capture-123"
  }
}
```

## Executive Assistant Tools

The v1 assistant surface is available through `smesh` command families that
mirror the MCP tools: `tasks`, `reminders`, `contacts`, `preferences`, `briefs`,
and `calendar`.

All assistant writes require mesh context from `--mesh-id`, `SMESH_MESH_ID`, or
the selected profile. JSON output is the automation contract.

Examples:

```bash
smesh --json --mesh-id "$SMESH_MESH_ID" \
  tasks create "Send revised contract to ACME" \
  --description "Use May 8 redlines and confirm billing address." \
  --due-at 2026-05-12T21:00:00Z \
  --priority high \
  --tag legal \
  --source-type linear_issue \
  --source-id SCI-93

smesh --json --mesh-id "$SMESH_MESH_ID" \
  reminders due-soon --window PT24H

smesh --json --mesh-id "$SMESH_MESH_ID" \
  preferences set \
  --scope project:project-acme-renewal \
  --key briefs.daily.max_length \
  --value short \
  --update-rule confirm_on_conflict

smesh --json --mesh-id "$SMESH_MESH_ID" \
  calendar events list \
  --from 2026-05-12T00:00:00Z \
  --to 2026-05-13T00:00:00Z
```

The CLI sends these calls to `/api/cli/*` routes and forwards the stable
snake_case JSON response unchanged. Agents should keep their own retry and
audit logs keyed by returned record ids and any supplied `idempotency_key`.

## OpenClaw/Codex Capture Hook

The example workflow in
[`../examples/openclaw-codex-capture-note.sh`](../examples/openclaw-codex-capture-note.sh)
captures an agent note and prints the structured enqueue response unchanged.

Required environment:

```bash
export SMESH_AGENT_MODE=1
export SMESH_TOKEN="<access-token>"
export SMESH_MESH_ID="<mesh-id>"
```

Usage:

```bash
apps/cli-rust/examples/openclaw-codex-capture-note.sh \
  "Codex completed SCI-81 validation plan and opened a PR."
```

Codex can use the same contract from Python without shell parsing:

```python
import json
import os
import subprocess

result = subprocess.run(
    ["smesh", "capture", "text", "Codex workpad update", "--tag", "codex"],
    check=False,
    capture_output=True,
    text=True,
    env={**os.environ, "SMESH_AGENT_MODE": "1"},
)
payload = json.loads(result.stdout)
if result.returncode != 0:
    raise RuntimeError(payload["error"])
print(payload["operation_id"])
```
