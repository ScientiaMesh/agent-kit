# ScientiaMesh Agent Kit

Public distribution kit for ScientiaMesh agents.

This repository contains installable AgentSkills, `smesh` CLI installation
metadata, and human-readable setup docs for agents that use ScientiaMesh as
durable memory.

## Quick Start

Install the `smesh` CLI from the public GitHub release:

```bash
curl -fsSL https://raw.githubusercontent.com/ScientiaMesh/agent-kit/main/install.sh | sh
smesh --version
```

Install the Executive Assistant skill from this repository:

```bash
git clone https://github.com/ScientiaMesh/agent-kit.git
cp -R agent-kit/skills/scientiamesh-ea "$CODEX_HOME/skills/"
```

Or download the packaged skill bundle:

```bash
curl -fsSLO https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/scientiamesh-ea-latest.tar.gz
tar -xzf scientiamesh-ea-latest.tar.gz
```

## Contents

- `skills/scientiamesh-ea/` - Executive Assistant AgentSkill.
- `docs/install-smesh.md` - CLI installation and verification.
- `docs/install-ea-skill.md` - Skill installation options.
- `docs/agent-setup.md` - Minimal environment setup for agents.
- `install.sh` - Stable `smesh` installer script.
- `releases/smesh/` - Static release metadata mirror.

## Stable URLs

- GitHub release: `https://github.com/ScientiaMesh/agent-kit/releases/tag/smesh-latest`
- CLI manifest: `https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/smesh-latest.json`
- CLI installer: `https://raw.githubusercontent.com/ScientiaMesh/agent-kit/main/install.sh`
- EA skill manifest: `https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/scientiamesh-ea-latest.json`
- EA skill bundle: `https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/scientiamesh-ea-latest.tar.gz`

## Status

The public release channel currently publishes Linux x64, macOS arm64, and
Windows x64 binaries.
