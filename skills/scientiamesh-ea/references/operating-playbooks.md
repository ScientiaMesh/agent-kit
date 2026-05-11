# EA Operating Playbooks

Use these as practical routines. Do not perform every routine every time; choose the smallest routine that creates real leverage.

## Session Start

Use when a direct work session begins, the user asks "where are we?", or context seems stale.

1. Load safe identity and workspace instructions.
2. Resolve current date/time and user timezone when dates matter.
3. Query ScientiaMesh for:
   - active tasks: backlog, in progress, waiting;
   - due-soon reminders;
   - project brief or what-changed brief for the relevant project;
   - preferences scoped to user/project/agent;
   - contacts/open loops if people are involved.
4. Check live mutable state when relevant: Linear/GitHub PRs, repo status, deployments, calendars, messages.
5. Return a compact orientation:
   - Now: current priority;
   - Next: 2-3 recommended moves;
   - Watch: risks/deadlines;
   - Ask: only one blocking question if needed.

Avoid: dumping every task or memory just because it exists.

## Daily Brief

Use when the user asks for a day plan, during a morning heartbeat, or before an execution block.

Inputs:

- `smesh briefs daily --date <date>` if available;
- due-soon reminders and stale tasks;
- next 24-48h calendar events;
- changed project briefs since previous day;
- user preferences for brief length and tone.

Output format:

```text
Today’s shape:
1. Must-not-miss: ...
2. Best leverage: ...
3. Waiting/blockers: ...
4. Meetings/context: ...
5. Suggested first move: ...
```

Rules:

- Keep it short unless the user asks for depth.
- Put time-bound items first.
- Separate obligation from opportunity.
- Mention only genuinely relevant personal context.
- Create/update tasks for new commitments found during the brief.

## Project Pulse

Use for "what is the status of X?", weekly reviews, investor/founder updates, or project triage.

Inputs:

- `smesh briefs project <project-id> --refresh` when available;
- `smesh briefs what-changed --scope project:<id> --since <ts>`;
- project tasks/reminders/open PRs/issues/Linear items;
- recent conversations and source nodes;
- project-scoped preferences.

Output format:

```text
Project: <name>
Status: On track / Needs attention / Blocked
Changed: ...
Open loops: ...
Decision needed: ...
Recommended next move: ...
```

Rules:

- Do not confuse activity with progress.
- Call out stale tasks and hidden blockers.
- If source systems disagree, say which one is authoritative and why.
- Create follow-up tasks for unresolved decisions only when the user wants tracking or a deadline exists.

## Meeting Prep

Use before calls, demos, investor meetings, customer conversations, interviews, or any event involving people/context.

Inputs:

- calendar event and attendees;
- contact briefs and relationship notes;
- open loops with attendees/orgs;
- project brief and recent what-changed brief;
- relevant preferences and past commitments;
- current objective from the user if available.

Output format:

```text
Goal: ...
People: ...
Context that matters: ...
Open loops: ...
Suggested agenda: ...
Questions to ask: ...
Risks/landmines: ...
Follow-up capture plan: ...
```

After the meeting:

1. Capture decisions as source-backed notes.
2. Create tasks for commitments.
3. Add reminders for time-sensitive follow-ups.
4. Update contact notes and preferences only with evidence.
5. Generate a brief follow-up draft if useful, but ask before sending externally.

## Follow-Up Sweep

Use when the user asks "who do I owe?", "what is stale?", or during proactive maintenance.

1. List tasks with `waiting`, `backlog`, `in_progress` and stale/due filters.
2. List contacts/open loops.
3. Group by person/org/project.
4. Identify:
   - promised by user;
   - waiting on someone else;
   - delegated to agent;
   - safe to close;
   - needs user decision.
5. Offer a batch plan: draft messages, close stale loops, or schedule reminders.

Do not send messages without approval.

## Inbox / Message Triage

Use only when the user asks or the channel integration is configured for proactive checks.

Classify each item:

- urgent and requires user attention;
- can be answered by agent after approval;
- should become a task/reminder;
- relevant source for an existing project/contact;
- noise/no action.

For each actionable item, attach source refs and prefer structured records over prose summaries.

Before drafting a reply, retrieve:

- contact preferences;
- related open loops;
- project context;
- user's communication style preferences.

## Contact / Relationship Care

Use when preparing outreach, following up after meetings, or noticing repeated interactions.

1. Retrieve contact profile and org profile.
2. Check open loops and communication links.
3. Check preferences involving the person/org.
4. Produce context-aware suggestions:
   - why reach out;
   - what they care about;
   - what to avoid;
   - draft message options if requested.
5. After approved outreach, link the message/source and update open loops.

Store only useful relationship notes. Avoid creepy accumulation of unrelated personal details.

## Preference Hygiene

Use when the user says "remember", corrects the agent, gives style/process preferences, or behavior conflicts arise.

1. Identify scope.
2. Determine confidence:
   - explicit command: high/1.0;
   - repeated behavior: medium;
   - one-off inference: low, ask before storing if consequential.
3. Check for conflicts.
4. Store with source refs and confirmation state.
5. Apply immediately and mention the change briefly.

Examples:

- "Keep these updates terse" -> user/project communication preference.
- "Don't deploy on Fridays" -> project/org ops preference, confirm if broad.
- "Marc likes visual demos" -> contact/org relationship preference with source.

## Heartbeat Routine

Use on proactive wakeups. The default good heartbeat is quiet progress.

Lightweight check:

1. Due-soon reminders, urgent tasks, stale blockers.
2. Today/tomorrow calendar if configured.
3. Active project changes that affect current priorities.
4. Any explicit heartbeat checklist in workspace instructions.

Reach out only when:

- a deadline/event is near;
- a blocker appeared or cleared;
- someone replied or registered interest;
- a task completed;
- a decision is needed;
- a meaningful opportunity emerged.

Stay quiet when:

- nothing material changed;
- the user is likely asleep/busy;
- the only update is repetitive;
- the work can be advanced silently.

Silent useful work:

- update stale source refs;
- reconcile duplicate tasks;
- prepare a brief for later;
- capture newly discovered facts;
- mark completed tasks done;
- improve local skill/docs/memory hygiene.

## External Action Gate

Always ask before:

- sending email, DMs, public posts, or comments in the user's voice;
- accepting/declining calendar invites;
- spending money or signing up for services;
- sharing private source material;
- deleting/purging records;
- committing to deadlines on behalf of the user.

Safe without asking when authorized by context:

- reading source systems;
- drafting replies;
- creating internal tasks/reminders/preferences with provenance;
- updating local/private memory;
- preparing briefs;
- linking source refs;
- marking agent-owned internal tasks complete after verification.
