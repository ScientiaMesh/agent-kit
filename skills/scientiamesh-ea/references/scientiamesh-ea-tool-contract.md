# ScientiaMesh EA Tool Contract

Source: SCI-92 / PR #270 as inspected on 2026-05-09. Treat this as the agent-facing operating contract. If the checked-in spec changes, prefer the latest repo spec over this summary.

## Core Contract

- Use `--json` for CLI automation. Human output is not the automation contract.
- Writes require a valid `mesh_id` unless a trusted profile resolves one.
- Outputs use stable snake_case JSON.
- MCP tools and CLI commands must call the same service layer and enforce the same auth, mesh, privacy, idempotency, and error behavior.
- Every write should include actor and source/provenance whenever possible.
- Do not infer permission to send emails/messages, spend money, accept invites, or make external commitments from task delegation alone.

Common MCP input envelope:

```json
{
  "mesh_id": "<uuid>",
  "actor": {"type": "agent", "id": "pixel"},
  "idempotency_key": "optional-stable-key"
}
```

Common successful MCP output:

```json
{
  "ok": true,
  "data": {},
  "meta": {"request_id": "req_...", "mesh_id": "<uuid>"}
}
```

Common error shape:

```json
{
  "ok": false,
  "error": {
    "code": "SMESH_MESH_REQUIRED",
    "message": "mesh_id is required for assistant tool writes.",
    "details": null,
    "retryable": false
  },
  "meta": {"request_id": "req_..."}
}
```

Important error codes include `SMESH_USAGE`, `SMESH_AUTH_REQUIRED`, `SMESH_MESH_REQUIRED`, `SMESH_CONFIG`, `SMESH_UNSUPPORTED`, `SMESH_NOT_FOUND`, and privacy/auth scope failures.

## Source References

Use source refs rather than copying private content. Preferred shape:

```json
{
  "type": "source_node|conversation|linear_issue|github_pr|calendar_event|file|markdown_import|url",
  "id": "stable-source-id",
  "label": "optional human label",
  "span": {"start": 220, "end": 410},
  "url": null,
  "captured_at": "2026-05-09T13:20:00Z"
}
```

Rules:

- Include `source_refs` for tasks, reminders, contacts, preferences, and brief evidence.
- Use concise spans or ids instead of full private messages when possible.
- Use deterministic ids/idempotency keys for repeated agent routines.

## Tasks

Assistant tasks are human work items, not worker/Synapse jobs.

CLI:

```bash
smesh tasks create [options] <title>
smesh tasks list [filters]
smesh tasks get <task-id>
smesh tasks update <task-id> [fields]
smesh tasks complete <task-id> [--note <text>]
smesh tasks delegate <task-id> --to-agent <id>|--to-user <id>|--to-contact <id>
smesh tasks attach-source <task-id> --source-type <type> --source-id <id> [--span-start N --span-end N]
```

MCP:

- `smesh_tasks_create`
- `smesh_tasks_list`
- `smesh_tasks_get`
- `smesh_tasks_update`
- `smesh_tasks_complete`
- `smesh_tasks_delegate`
- `smesh_tasks_attach_source`

Task fields to preserve:

- `title`, `description`, `status`, `priority`, `due_at`, `start_at`, `timezone`, `recurrence`
- `assignee_user_id`, `delegated_to`, `delegation_status`
- `stale_at`, `tags`, `source_refs`, `confidence`, `created_via`, timestamps

Status enum: `backlog`, `in_progress`, `waiting`, `done`, `canceled`.
Priority enum: `low`, `normal`, `high`, `urgent`.

Create example:

```bash
smesh --mesh-id <mesh-id> --json tasks create "Send revised contract to ACME" \
  --description "Use May 8 redlines and confirm billing address." \
  --due-at 2026-05-12T21:00:00Z \
  --priority high \
  --tag legal \
  --source-type source_node \
  --source-id source-meeting-2026-05-08
```

Before starting a session:

```bash
smesh --json tasks list \
  --mesh-id <mesh-id> \
  --status backlog,in_progress,waiting \
  --due-before <iso-ts> \
  --stale-before <iso-ts> \
  --limit 20
```

Use idempotency keys like `linear:SCI-92:pr-description`, `heartbeat:<date>:daily-brief`, or `conversation:<id>:<slug>`.

## Reminders

CLI:

```bash
smesh reminders create [--task-id <id>] <title> [schedule options]
smesh reminders list [--due-before <ts>] [--due-after <ts>] [--state <states>]
smesh reminders due-soon [--window PT24H]
smesh reminders snooze <reminder-id> --until <ts>|--duration <iso-duration>
smesh reminders complete <reminder-id> [--complete-task]
smesh reminders dismiss <reminder-id>
```

MCP:

- `smesh_reminders_create`
- `smesh_reminders_list`
- `smesh_reminders_due_soon`
- `smesh_reminders_snooze`
- `smesh_reminders_complete`
- `smesh_reminders_dismiss`

Reminder states: `scheduled`, `due`, `snoozed`, `completed`, `dismissed`, `disabled`.

Rules:

- A reminder can be task-linked or standalone.
- `complete` completes the reminder only unless `complete_task=true` is explicit.
- V1 escalation is internal surfacing: daily brief, project brief, task list priority, or agent notification.
- Do not use email/SMS/push unless a separate integration is installed and enabled.
- Unsupported recurrence must fail explicitly instead of storing ambiguous schedules.

Due-soon check:

```bash
smesh --json reminders due-soon --mesh-id <mesh-id> --window PT24H
```

## Contacts

Contacts include people, orgs, roles, relationship notes, communication history links, and open loops.

CLI:

```bash
smesh contacts people create --name <name> [--org <org-id>] [--email <email>]
smesh contacts people list [--query <text>] [--open-loops]
smesh contacts people get <person-id>
smesh contacts people note add <person-id> --body <text> [--source-id <id>]
smesh contacts orgs create --name <name> [--domain <domain>]
smesh contacts orgs list [--query <text>]
smesh contacts orgs get <org-id>
smesh contacts links add <person-or-org-id> --source-type <type> --source-id <id>
smesh contacts open-loops list [--person <id>|--org <id>]
```

MCP:

- `smesh_contacts_people_create`
- `smesh_contacts_people_list`
- `smesh_contacts_people_get`
- `smesh_contacts_people_update`
- `smesh_contacts_orgs_create`
- `smesh_contacts_orgs_list`
- `smesh_contacts_orgs_get`
- `smesh_contacts_note_add`
- `smesh_contacts_link_source`
- `smesh_contacts_open_loops_list`

Contact brief input:

```bash
smesh --json contacts people get <person-id> \
  --mesh-id <mesh-id> \
  --include open_loops,communication_links,preferences
```

Rules:

- Preserve contact assertion evidence; do not flatten away provenance.
- Keep relationship notes concise and useful.
- Mark sensitive notes private unless the user says otherwise.
- Open loops should link to task ids when possible.

## Preferences

Preferences are scoped, evidenced claims. They are also the right place for
human-set operating rules: communication style, boundaries, privacy norms,
workflow defaults, proactivity thresholds, and agent collaboration preferences.
Do not use preferences to weaken non-negotiable safety/platform policy.

CLI:

```bash
smesh preferences set --scope user|org|project|mesh|agent --key <key> --value <value> [--confidence N]
smesh preferences list [--scope <scope>] [--domain <domain>] [--min-confidence N]
smesh preferences get <preference-id>
smesh preferences confirm <preference-id>
smesh preferences revoke <preference-id> [--reason <text>]
smesh preferences evidence <preference-id>
```

MCP:

- `smesh_preferences_set`
- `smesh_preferences_list`
- `smesh_preferences_get`
- `smesh_preferences_confirm`
- `smesh_preferences_revoke`
- `smesh_preferences_evidence`

Set example:

```json
{
  "mesh_id": "<mesh-id>",
  "scope": {"type": "project", "id": "project-acme-renewal"},
  "key": "briefs.daily.max_length",
  "value": "short",
  "value_type": "enum",
  "polarity": "need",
  "confidence": 0.91,
  "source_refs": [{"type": "conversation", "id": "convo_..."}],
  "update_rule": "confirm_on_conflict"
}
```

Rules:

- Scope tightly. Do not turn a one-project preference into a global preference without evidence.
- Use `confirm_on_conflict` for inferred or conversational preferences.
- Use `last_confirmed_at` to distinguish confirmed from stale.
- Ask before overwriting contradictory preferences.
- For rule-like preferences, include polarity/enforcement metadata when the API supports it: `must`, `must_not`, `prefer`, `avoid`, `ask_first`; `advisory`, `require_confirmation`, or `deny_without_permission`.

See `preference-taxonomy.md` for the recommended namespaces for rules and boundaries.

## Briefs

CLI:

```bash
smesh briefs daily [--date YYYY-MM-DD] [--refresh]
smesh briefs project <project-id> [--since <ts>] [--refresh]
smesh briefs contact <person-or-org-id> [--refresh]
smesh briefs meeting-prep <event-id> [--refresh]
smesh briefs what-changed [--since <ts>] [--scope project:<id>|contact:<id>|mesh]
```

MCP:

- `smesh_briefs_daily`
- `smesh_briefs_project`
- `smesh_briefs_contact`
- `smesh_briefs_meeting_prep`
- `smesh_briefs_what_changed`

Daily brief:

```bash
smesh --json briefs daily --mesh-id <mesh-id> --date YYYY-MM-DD
```

What changed:

```bash
smesh --json briefs what-changed \
  --mesh-id <mesh-id> \
  --since <iso-ts> \
  --scope project:<id>
```

Briefs should include evidence refs for actionable recommendations. If evidence is weak, say so.

## Calendar

V1 calendar is a near-term event context surface, not full OAuth/sync/scheduling.

CLI:

```bash
smesh calendar events list --from <ts> --to <ts>
smesh calendar events get <event-id>
smesh calendar events upsert --file <json-file>
smesh calendar meeting-prep <event-id> [--refresh]
```

MCP:

- `smesh_calendar_events_list`
- `smesh_calendar_events_get`
- `smesh_calendar_events_upsert`

Rules:

- Store near-term event metadata needed for assistant work, not full historical archives.
- Attendees should link to contact ids when possible.
- Import/upsert events from available connectors; do not assume permission to modify external calendars.

## Migration From Markdown

Preferred later command:

```bash
smesh assistant import-markdown ./pixel-memory.md \
  --mesh-id <mesh-id> \
  --dry-run
```

Import rules:

- checked items become completed tasks only with `--include-completed`;
- unchecked items become candidate tasks;
- headings become tags only when deterministic;
- free-form notes become source refs or relationship notes only after confirmation;
- secrets and tokens must be redacted before upload;
- imported items use `source_refs.type = "markdown_import"`.
