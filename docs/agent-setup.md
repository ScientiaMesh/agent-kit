# Agent Setup

Agents using ScientiaMesh need three things:

1. The `smesh` CLI.
2. A ScientiaMesh auth token or configured `smesh` profile.
3. A target mesh id.

## Environment

```bash
export SMESH_MESH_ID="<mesh-id>"
export SMESH_TOKEN="<token>"
```

Then verify:

```bash
smesh --json auth status
smesh --json status
```

## Skill

Install the Executive Assistant skill:

```text
github:ScientiaMesh/agent-kit/skills/scientiamesh-ea
```

The skill itself documents the expected memory contract and common workflows.
