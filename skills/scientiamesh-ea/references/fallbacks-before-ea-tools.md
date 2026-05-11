# Fallbacks Before SCI-92 EA Tools Exist

Use this only when first-class EA commands/MCP tools are unavailable. The goal is to preserve behavior and provenance now while making migration to the official tools easy later.

## Availability Check

Run:

```bash
python3 /home/pixel/.openclaw/workspace/skills/scientiamesh-ea/scripts/smesh_ea_status.py --mesh-id <mesh-id>
```

If it reports missing commands, use the fallbacks below.

## Retrieval Fallback

Use existing ScientiaMesh retrieval before local markdown when possible.

Pixel default patterns:

```bash
smesh context get --agent Pixel --meshid <mesh-id>
smesh topics query --topic scientiamesh --topic pixel --recent --format compact --summary
smesh topics activity --topic scientiamesh --format compact
```

For a project:

```bash
smesh topics query --topic scientiamesh --topic pixel --topic project/scientiamesh --recent --format compact --summary
```

For deeper grouping:

```bash
smesh topics query --topic scientiamesh --recent --format compact --group-by node_type
```

If ScientiaMesh retrieval fails or is thin, use workspace docs that are explicitly source-of-truth for that domain, then local memory files as a fallback.

## Capture Fallback

When tools do not exist yet, capture structured text with consistent topics.

Base instructions:

```text
Store as long-term agent memory. Preserve timeline facts and canonical names. #scientiamesh #pixel #project/scientiamesh #type:note #status:open
```

Task-like item:

```bash
smesh capture text "TASK: <title>
Owner: <user/agent/person>
Status: open
Due: <iso-date-or-none>
Priority: <low|normal|high|urgent>
Source: <source-ref>
Notes: <concise context>" \
  --instructions "Store as assistant task candidate with source provenance. #scientiamesh #pixel #project/scientiamesh #type:todo #status:open #priority:<low|med|high>"
```

Preference-like item:

```bash
smesh capture text "PREFERENCE: <key/value>
Scope: <user|project|org|mesh|agent>:<id-or-name>
Confidence: <0..1>
Source: <source-ref>
Update rule: confirm_on_conflict" \
  --instructions "Store as scoped preference candidate with evidence and confirmation semantics. #scientiamesh #pixel #type:preference #status:open"
```

Contact note:

```bash
smesh capture text "CONTACT NOTE: <person/org>
Role: <role-if-known>
Relationship note: <concise useful fact>
Open loop: <task/follow-up-if-any>
Source: <source-ref>" \
  --instructions "Store as contact/relationship memory with provenance. #scientiamesh #pixel #type:contact-note #status:open"
```

Decision:

```bash
smesh capture text "DECISION: <decision>
Context: <why>
Applies to: <project/person/scope>
Source: <source-ref>
Follow-up: <if any>" \
  --instructions "Store as durable decision memory. #scientiamesh #pixel #type:decision #status:done"
```

Completion/update:

```bash
smesh capture text "UPDATE: <item>
Previous status: open
New status: done|blocked|waiting
Evidence: <source-ref>
Notes: <concise>" \
  --instructions "Store as status update and dedupe against existing memory. #scientiamesh #pixel #type:update #status:done"
```

## Local Markdown Fallback

Use local files only when:

- first-class EA tools are missing;
- ScientiaMesh capture/retrieval is unavailable or insufficient;
- workspace docs are the established source of truth for the domain;
- the note is explicitly local/private operational context.

Rules:

- Update the relevant source-of-truth doc, not a random scratch note.
- Keep dates concrete.
- Do not store secrets unless explicitly instructed.
- If later importing, use `markdown_import` source refs and dry-run first.

## Migration Breadcrumbs

For every fallback record, include enough structure to map to the future v1 schema:

- kind: task/reminder/contact/preference/brief/calendar/decision/update;
- title/key/name;
- owner/scope;
- status;
- due/stale time if any;
- source reference;
- confidence;
- visibility/private note if relevant;
- idempotency hint.

Suggested idempotency key patterns:

- `linear:<issue-id>:<slug>`
- `github:<owner>/<repo>#<number>:<slug>`
- `conversation:<message-id>:<slug>`
- `calendar:<event-id>:<slug>`
- `heartbeat:<yyyy-mm-dd>:<routine>`
- `project:<project-id>:<slug>`

## What Not To Do

- Do not silently scrape arbitrary local files into ScientiaMesh.
- Do not upload secrets/tokens.
- Do not create duplicate TODOs on each heartbeat.
- Do not mark checked markdown items complete unless explicitly importing completed items.
- Do not over-store private personal details that will not help future assistance.
- Do not treat fallback captures as a substitute for implementing the real tools.
