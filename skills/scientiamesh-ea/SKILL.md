# ScientiaMesh Executive Assistant

Use this skill when acting as an executive assistant that needs durable,
source-linked memory in ScientiaMesh.

## Requirements

- `smesh` CLI compatible with `>=0.1.0 <1.0.0`
- ScientiaMesh API compatible with the CLI v1 preview routes
- Auth from `SMESH_TOKEN`, `SMESH_API_KEY`, or a configured `smesh` profile
- Mesh context from `SMESH_MESH_ID` or `--mesh-id`

## Install `smesh`

Install or update the Rust CLI before using this skill:

```bash
curl -fsSL https://raw.githubusercontent.com/ScientiaMesh/agent-kit/main/install.sh | sh
```

Agents that need to select a platform themselves should read the public
manifest:

```bash
curl -fsSL https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/smesh-latest.json
```

Direct platform URLs use this shape:

```text
https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/smesh-linux-x64
https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/smesh-macos-arm64
https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/smesh-windows-x64.exe
```

After installation, confirm the CLI is available:

```bash
smesh --version
```

## Operating Rules

1. Prefer `smesh --json` for all reads and writes so output is machine-stable.
2. Never infer a mesh id. Ask for it or use the configured profile/context.
3. Capture decisions, commitments, meeting notes, and durable preferences with
   enough source text for future retrieval.
4. Use search or ask before summarizing historical context unless the user
   explicitly provides all needed source material in the current turn.
5. Keep private or sensitive material scoped to the active mesh. Do not copy
   tokens, secrets, or unrelated personal data into captures.

## Common Commands

Check local configuration:

```bash
smesh --json auth status
smesh --json status
```

Capture a note:

```bash
smesh --json --mesh-id "$SMESH_MESH_ID" capture text "$NOTE" \
  --instructions "Store this as executive-assistant working memory."
```

Retrieve context:

```bash
smesh --json --mesh-id "$SMESH_MESH_ID" search --top-k 8 "$QUERY"
smesh --json --mesh-id "$SMESH_MESH_ID" ask "$QUESTION"
```

Install or update this skill through the skill registry:

```bash
smesh --json --mesh-id "$SMESH_MESH_ID" skills set \
  --name scientiamesh-ea \
  --file SKILL.md \
  --format markdown
```

## References

- `references/workflows.md`
- `references/memory-contract.md`
