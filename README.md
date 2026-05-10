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
curl -fsSLO https://download.scientiamesh.app/skills/scientiamesh-ea/latest.tar.gz
tar -xzf latest.tar.gz
```

## Contents

- `skills/scientiamesh-ea/` - Executive Assistant AgentSkill.
- `docs/install-smesh.md` - CLI installation and verification.
- `docs/install-ea-skill.md` - Skill installation options.
- `docs/agent-setup.md` - Minimal environment setup for agents.
- `install.sh` - Stable `smesh` installer script.
- `releases/smesh/` - Static release metadata mirror.

## Stable URLs

- CLI manifest: `https://download.scientiamesh.app/smesh/latest.json`
- CLI installer: `https://download.scientiamesh.app/install.sh`
- GitHub release: `https://github.com/ScientiaMesh/agent-kit/releases/tag/smesh-latest`
- EA skill manifest: `https://download.scientiamesh.app/skills/scientiamesh-ea/latest.json`
- EA skill bundle: `https://download.scientiamesh.app/skills/scientiamesh-ea/latest.tar.gz`

## Status

The Linux x64 `smesh` binary is currently published. macOS and Windows binaries
are expected to be produced by GitHub Actions release automation.
