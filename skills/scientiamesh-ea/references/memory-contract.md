# Memory Contract

ScientiaMesh memory used by this skill should be:

- Mesh-scoped: every command must resolve an explicit mesh id.
- Source-linked: captured text should include the provenance needed to audit it.
- Minimal: store the durable fact, decision, or task rather than an entire chat
  unless the transcript itself is the source of truth.
- Verifiable: search or ask the mesh before making claims about historical
  context.
- Private by default: do not widen access or repeat sensitive data unless the
  user explicitly asks for it and the active mesh is appropriate.

Recommended capture tags:

- `executive-assistant`
- `decision`
- `commitment`
- `meeting-note`
- `preference`
