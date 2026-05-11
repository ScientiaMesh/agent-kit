# Source Synapse Assertions And Agent CRUD Workflows

Keep two concepts separate:

- **Agents** are authorized actors. They have scoped authority in the meshes they can access and should have first-class tools to directly create, read, update, delete, attach, and relate canonical information.
- **Synapses** are internal ScientiaMesh intelligence/processors. When a synapse processes a new source and infers a project, task, contact, preference, or relationship, it should usually write an assertion for an agent or user to confirm, deny, merge, or delegate.

This distinction matters. ScientiaMesh is agent-first: agents need dependable CRUD tools over canonical mesh data. Assertions are the trust boundary for internal inference, not a replacement for agent authority.

## Actor Model

### User

The user owns or participates in meshes. They can confirm, deny, delegate, revoke, and override according to mesh permissions.

### Agent

An agent is an authorized mesh actor, e.g. Pixel. In meshes it can access, an agent may directly CRUD canonical records when the user's policy, role, and current instruction allow it.

Agent writes should still carry:

- `actor = {"type":"agent","id":"..."}`;
- mesh/tenant scoping;
- source refs when the write is based on evidence;
- audit trail;
- idempotency keys for repeated routines.

Agents may also manage assertion queues: summarize, attach evidence, merge duplicates, recommend confirmation, confirm/deny when delegated, and create canonical records from confirmed assertions.

### Synapse

A synapse processes sources internally: extraction, summarization, entity detection, project detection, task detection, contact detection, preference detection, and similar intelligence.

A synapse should not silently turn uncertain inference into canonical business truth. It should emit assertions with evidence and confidence.

Synapse writes should usually carry:

- `actor = {"type":"synapse","id":"project_detection@..."}`;
- source refs and evidence spans;
- confidence;
- assertion status;
- candidate links to existing canonical records.

## Canonical Records

Canonical records are the data agents use to operate:

- `projects` / `project_states`: confirmed Project entities.
- `tasks` / `task_states`: confirmed user work items.
- `contacts` / `contact_states`: confirmed or accepted people/org/contact records.
- `preferences` / `preference_states`: active user/org/project/agent preferences.
- Graph nodes: `(:Project)`, `(:Task)`, `(:Person)`, `(:Org)`, `(:Preference)` for retrieval, briefs, relationships, and evidence traversal.

Agents should have direct tools for canonical CRUD in authorized meshes. Example surfaces:

```bash
smesh projects create|get|list|update|archive|delete
smesh tasks create|get|list|update|complete|delete
smesh contacts people create|get|list|update|delete
smesh contacts orgs create|get|list|update|delete
smesh preferences set|get|list|confirm|revoke
```

MCP should mirror those canonical tools, e.g. `smesh_projects_create`, `smesh_tasks_update`, `smesh_contacts_people_update`.

Destructive or external-impact actions still obey human preferences/policy. Mesh access means the agent is technically authorized; it does not automatically mean the user wants a specific irreversible action.

## Assertion Records

Assertion records are evidence-backed hypotheses produced primarily by synapses, and sometimes by agents acting in triage mode.

- `project_assertions`: candidate projects inferred from sources.
- `task_assertions`: candidate tasks inferred from sources.
- `contact_assertions`: candidate people/org/contact facts inferred from sources.
- `preference_assertions`: candidate preferences inferred from sources.

Assertions have lifecycle state:

```text
pending -> confirmed -> canonical record created/linked
pending -> denied -> kept as negative evidence
pending -> delegated -> authorized agent manages/triages within explicit scope
pending -> merged/superseded -> duplicate or refined candidate
```

## Why Task Assertions Matter

Directly creating tasks from every internal source inference creates noise. Task assertions let ScientiaMesh say:

> "This looks like a task. Should it become tracked work?"

That gives business users and agents a clean assistant workflow:

- review proposed tasks;
- confirm real ones;
- deny noise;
- merge duplicates;
- attach to projects;
- delegate routine triage to an agent;
- learn patterns over time.

Important nuance: an authorized agent can still create a canonical task directly when the user asks or policy permits it. Assertions are for uncertain inference, especially from synapse processing.

## Project Assertions

A project assertion proposes a new or existing project context.

Suggested fields:

```json
{
  "id": "project_assertion_...",
  "mesh_id": "<uuid>",
  "asserted_name": "ScientiaMesh EA Tools",
  "aliases": ["EA tools", "assistant tools"],
  "description": "First-class assistant task/reminder/contact/preference/brief tools.",
  "candidate_project_id": null,
  "confidence": 0.87,
  "status": "pending",
  "asserted_by": {"type": "synapse", "id": "project_detection@1.0.0"},
  "source_refs": [{"type": "conversation", "id": "..."}],
  "evidence_summary": "Repeated discussion of a coherent workstream with tasks, specs, and PRs.",
  "created_at": "2026-05-09T04:23:00Z",
  "updated_at": "2026-05-09T04:23:00Z"
}
```

User or delegated agent actions:

- confirm as new project;
- attach to existing project;
- deny;
- merge with another assertion;
- delegate future similar assertions to an agent within rules.

## Task Assertions

A task assertion proposes work that might need tracking.

Suggested fields:

```json
{
  "id": "task_assertion_...",
  "mesh_id": "<uuid>",
  "title": "Write spec for Project node/entity and assertion workflow",
  "description": "Define Project as DB entity + graph node with assertion confirmation flow.",
  "candidate_task_id": null,
  "candidate_project_id": "project_scientiamesh",
  "project_assertion_id": null,
  "assignee_hint": {"type": "agent", "id": "pixel"},
  "priority_hint": "high",
  "due_at_hint": null,
  "confidence": 0.91,
  "status": "pending",
  "asserted_by": {"type": "synapse", "id": "task_detection@1.0.0"},
  "source_refs": [{"type": "conversation", "id": "..."}],
  "evidence_summary": "Source contains a likely follow-up or commitment.",
  "created_at": "2026-05-09T04:27:00Z",
  "updated_at": "2026-05-09T04:27:00Z"
}
```

User or delegated agent actions:

- confirm -> create canonical task;
- attach to project before confirm;
- confirm without project;
- deny;
- merge into existing task;
- delegate assertion management to an agent.

## Optional Task-To-Project Attachment

A canonical task may be:

- attached to a confirmed project;
- attached to a pending project assertion;
- deliberately unprojected;
- auto-suggested for project attachment later.

Rules:

- Do not require every task to belong to a project.
- Prefer project attachment when it improves briefs, contacts, preferences, or open-loop tracking.
- Keep unclassified tasks visible in daily/inbox views.
- Let users or delegated agents bulk triage: attach, leave unprojected, deny, or delegate.

Graph relationships:

```text
(:Task)-[:BELONGS_TO]->(:Project)
(:TaskAssertion)-[:CANDIDATE_FOR]->(:Project|:ProjectAssertion)
(:TaskAssertion)-[:EXTRACTED_FROM]->(:Source|:Conversation|:CalendarEvent)
(:ProjectAssertion)-[:EXTRACTED_FROM]->(:Source|:Conversation|:CalendarEvent)
```

## Agent Management Tools

Agent-first tools should include both canonical CRUD and assertion queue management.

Project assertions:

```bash
smesh project-assertions list --status pending --mesh-id <mesh-id> --json
smesh project-assertions confirm <id> --mesh-id <mesh-id> [--name <canonical-name>]
smesh project-assertions attach <id> --project-id <project-id> --mesh-id <mesh-id>
smesh project-assertions deny <id> --reason <text> --mesh-id <mesh-id>
smesh project-assertions merge <id> --into <other-id-or-project-id> --mesh-id <mesh-id>
```

Task assertions:

```bash
smesh task-assertions list --status pending --mesh-id <mesh-id> --json
smesh task-assertions confirm <id> --mesh-id <mesh-id> [--project-id <project-id>]
smesh task-assertions deny <id> --reason <text> --mesh-id <mesh-id>
smesh task-assertions merge <id> --task-id <task-id> --mesh-id <mesh-id>
smesh task-assertions delegate-triage --to-agent <agent-id> --scope <scope> --mesh-id <mesh-id>
```

MCP tools should mirror these with names such as:

- `smesh_project_assertions_list`
- `smesh_project_assertions_confirm`
- `smesh_project_assertions_deny`
- `smesh_task_assertions_list`
- `smesh_task_assertions_confirm`
- `smesh_task_assertions_deny`
- `smesh_task_assertions_delegate_triage`

## Delegating Assertion Management To Agents

A user may delegate assertion management to an agent. This is separate from synapse inference.

Delegation should be explicit and scoped:

```json
{
  "scope": {"type": "project", "id": "scientiamesh"},
  "agent_id": "pixel",
  "allowed_actions": ["list", "summarize", "group_duplicates", "attach_sources", "suggest_project", "deny_low_confidence_noise", "confirm_above_threshold"],
  "requires_confirmation": ["create_canonical_project", "delete", "external_commitment"],
  "auto_confirm_threshold": 0.97,
  "max_auto_actions_per_day": 20,
  "source_refs": [{"type": "conversation", "id": "..."}]
}
```

Start conservative:

1. Synapses emit assertions from source processing.
2. Agents list and summarize assertion queues.
3. Agents merge duplicates and attach sources when reversible.
4. Agents propose confirmations or create canonical records directly when the user asks.
5. Users delegate confirm/deny rules for narrow scopes.
6. High-confidence auto-confirm can come later, with audit and rollback.

## Learning Loop

Every confirmation/denial becomes training signal:

- confirmed assertion -> positive example for the synapse and agent triage policy;
- denied assertion -> negative example;
- merged assertion -> duplicate/alias learning;
- project attach/detach -> project classification signal;
- delegated action correction -> agent rule refinement.

Store these signals as evidence, not hidden magic. The user or agent should be able to inspect why a future assertion was created, ignored, or auto-confirmed.

## Brief And EA Impact

Assertions improve the EA experience:

- Daily brief can include "candidate tasks awaiting triage" separately from confirmed obligations.
- Project brief can include pending project/task assertions that may matter.
- Meeting prep can show likely follow-ups as candidates until confirmed.
- Preferences can scope behavior by confirmed project or project assertion.
- Contacts can expose open-loop candidates without pretending they are commitments.

Canonical CRUD tools keep the agent useful:

- When the user explicitly asks Pixel to track a task, Pixel should create the task, not just assert it.
- When a synapse extracts a likely task from a document, it should assert it for review.
- When the user delegates triage, Pixel can convert safe assertions into canonical records according to scope and policy.

## Guardrails

- Never confuse synapse inference with agent authority.
- Never bury user-confirmed tasks among unconfirmed assertions.
- Do not let assertions spam the user; batch them into triage views.
- Do not auto-create canonical projects/tasks from low-confidence synapse evidence.
- Keep denial history so the same noisy assertion does not return repeatedly.
- Treat delegation as permission to manage the queue, not permission to make external commitments.
- Preserve audit fields so every canonical write says whether it came from user instruction, agent action, or confirmed synapse assertion.
