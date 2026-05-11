# Preference Taxonomy For Human Rules And Boundaries

Use ScientiaMesh preferences as the editable, evidenced layer for human-set rules, boundaries, style, and operating norms.

Important distinction:

- **Preferences** are user/org/project/agent rules that the human can inspect, confirm, revoke, scope, and evolve.
- **Hard policy/permissions** are non-negotiable safety, platform, legal, or capability gates. Do not reduce these to preferences that an agent can ignore or override.

A perfect EA uses both: hard policy for safety rails, preferences for the human's desired operating contract.

## Preference Shape

Every rule-like preference should carry:

```json
{
  "scope": {"type": "user|project|org|mesh|agent|surface|contact", "id": "..."},
  "key": "agent.boundaries.external_messages.require_approval",
  "value": true,
  "value_type": "boolean|string|enum|json",
  "polarity": "must|must_not|prefer|avoid|allow|ask_first",
  "confidence": 1.0,
  "status": "active|revoked|superseded|stale",
  "source_refs": [{"type": "conversation", "id": "..."}],
  "last_confirmed_at": "2026-05-09T04:14:00Z",
  "update_rule": "confirm_on_conflict"
}
```

Recommended additions beyond the base SCI-92 contract:

- `polarity`: distinguish must, must_not, prefer, avoid, allow, ask_first.
- `sensitivity`: low, normal, sensitive, secret-adjacent.
- `enforcement`: advisory, ask_first, require_confirmation, deny_without_permission.
- `surface`: direct_chat, group_chat, public, customer, coding_agent, heartbeat.
- `applies_to_tools`: optional list, e.g. message, email, browser, exec, deploy.

## Key Namespaces

Use namespaced keys so agents can query by category.

### Communication

- `communication.tone.default`
- `communication.length.default`
- `communication.length.daily_brief`
- `communication.style.no_em_dash`
- `communication.group_chat.privacy_mode`
- `communication.discord.response_mode`
- `communication.progress_updates.frequency`

Examples:

```json
{"key":"communication.length.default","value":"concise","polarity":"prefer"}
{"key":"communication.group_chat.privacy_mode","value":"public_safe","polarity":"must"}
```

### External Action Boundaries

- `agent.boundaries.external_messages.require_approval`
- `agent.boundaries.public_posts.require_approval`
- `agent.boundaries.email_send.require_approval`
- `agent.boundaries.calendar_invites.require_approval`
- `agent.boundaries.spending.require_approval`
- `agent.boundaries.deployment.allow_when_requested`
- `agent.boundaries.destructive_actions.require_confirmation`

Examples:

```json
{"key":"agent.boundaries.external_messages.require_approval","value":true,"polarity":"must","enforcement":"require_confirmation"}
{"key":"agent.boundaries.destructive_actions.require_confirmation","value":true,"polarity":"must","enforcement":"require_confirmation"}
```

### Privacy And Memory

- `privacy.memory.personal_context.direct_only`
- `privacy.group_chat.private_memory_never_reveal`
- `privacy.secrets.never_store_unless_explicit`
- `memory.capture.decisions_immediately`
- `memory.capture.todos_immediately`
- `memory.provenance.require_source_refs`
- `memory.local_markdown.fallback_only`

Examples:

```json
{"key":"privacy.group_chat.private_memory_never_reveal","value":true,"polarity":"must","enforcement":"deny_without_permission"}
{"key":"memory.provenance.require_source_refs","value":true,"polarity":"must"}
```

### Proactivity And Interruptions

- `proactivity.heartbeat.silent_when_no_material_change`
- `proactivity.interrupt.deadline_threshold`
- `proactivity.interrupt.blockers_immediately`
- `proactivity.interrupt.repetitive_status_avoid`
- `proactivity.quiet_hours`
- `proactivity.background_work.allowed`

Examples:

```json
{"key":"proactivity.heartbeat.silent_when_no_material_change","value":true,"polarity":"must"}
{"key":"proactivity.interrupt.deadline_threshold","value":"PT2H","polarity":"prefer"}
```

### Workflows And Tooling

- `workflow.coding.delegate_to_codex_by_default`
- `workflow.coding.default_model`
- `workflow.coding.default_thinking`
- `workflow.github.prefer_pr_completion`
- `workflow.ops.source_of_truth`
- `workflow.scientiamesh.use_smesh_before_markdown`
- `workflow.tato.repo_source_of_truth`

Examples:

```json
{"key":"workflow.coding.delegate_to_codex_by_default","value":true,"polarity":"prefer"}
{"key":"workflow.scientiamesh.use_smesh_before_markdown","value":true,"polarity":"must"}
```

### Relationship / Contact Preferences

- `contact.<id>.communication.style`
- `contact.<id>.meeting.prep_depth`
- `org.<id>.followup.cadence`
- `relationship.notes.sensitivity_default`

Prefer contact-scoped preferences over global claims when the rule is about a specific person/org.

### Agent Identity And Collaboration

- `agent.pixel.tone`
- `agent.pixel.name`
- `agent.pixel.relationship_mode`
- `agent.pixel.opinions.encouraged`
- `agent.pixel.role.executive_assistant`

These should guide behavior, not override safety or privacy.

## Resolution Order

When multiple preferences apply, resolve from most specific to broadest:

1. hard system/platform policy;
2. explicit per-action user instruction in the current conversation;
3. surface-specific preference;
4. project/contact/org preference;
5. user preference;
6. mesh/team preference;
7. agent default.

If two active preferences conflict at the same specificity, ask the user or use the one with stronger confirmation and newer evidence when safe.

## Applying Preferences At Runtime

At the start of meaningful work:

1. Query preferences for relevant scopes: user, mesh, agent, surface, project, contact.
2. Apply hard boundaries first: external actions, privacy, destructive operations.
3. Apply style/process preferences next.
4. If a preference changes a recommendation, mention it briefly when helpful.
5. If evidence is stale or conflicting, ask one concise confirmation question.

For briefs and recommendations, include preference evidence only when it materially explains the output.

## Capturing Rules From Human Language

Map language to polarity:

- "always", "must", "never" -> `must` / `must_not`, ask before broad global scope if ambiguous.
- "prefer", "I like", "default to" -> `prefer`.
- "avoid", "I don't like" -> `avoid`.
- "ask me before" -> `ask_first` with `enforcement=require_confirmation`.
- "don't ever" -> `must_not` with high sensitivity; confirm if consequential and not already explicit.

Examples:

User: "Don’t send messages for me without asking."

```json
{
  "scope": {"type": "user", "id": "current"},
  "key": "agent.boundaries.external_messages.require_approval",
  "value": true,
  "polarity": "must",
  "enforcement": "require_confirmation",
  "confidence": 1.0,
  "update_rule": "confirm_on_conflict"
}
```

User: "For ScientiaMesh, use Linear as the source of truth."

```json
{
  "scope": {"type": "project", "id": "scientiamesh"},
  "key": "workflow.ops.source_of_truth",
  "value": "linear",
  "polarity": "must",
  "confidence": 1.0,
  "update_rule": "confirm_on_conflict"
}
```

User: "Keep the daily brief short unless something is on fire."

```json
{
  "scope": {"type": "user", "id": "current"},
  "key": "communication.length.daily_brief",
  "value": {"default":"short","expand_when":"urgent_or_blocked"},
  "value_type": "json",
  "polarity": "prefer",
  "confidence": 1.0
}
```

## Migration From Existing Agent Files

When importing AGENTS.md, SOUL.md, USER.md, MEMORY.md, or project docs:

1. Dry-run extraction first.
2. Classify each rule as hard policy, preference, task, contact note, or project fact.
3. Do not import secrets.
4. Preserve file/line source refs.
5. Mark inferred rules as candidates unless explicitly authored by the human.
6. Keep workspace-local rules in local docs until a user approves migrating them to ScientiaMesh.

Target command once import tooling exists:

```bash
smesh assistant import-markdown ./AGENTS.md --mesh-id <mesh-id> --dry-run --extract preferences
```

## Anti-Patterns

- Do not store platform safety rules as revocable user preferences.
- Do not make every observed habit a global rule.
- Do not apply private direct-chat preferences inside group/public contexts without checking privacy.
- Do not let low-confidence inferred preferences silently block useful work.
- Do not store contradictory rules without supersession or confirmation metadata.
