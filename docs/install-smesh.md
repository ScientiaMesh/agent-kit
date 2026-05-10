# Install `smesh`

`smesh` is the ScientiaMesh Rust CLI used by agents to read and write durable
memory.

## Auto Installer

```bash
curl -fsSL https://raw.githubusercontent.com/ScientiaMesh/agent-kit/main/install.sh | sh
smesh --version
```

By default, the installer writes to `$HOME/.local/bin/smesh`.

Override the install location:

```bash
SMESH_INSTALL_DIR="$HOME/bin" curl -fsSL https://raw.githubusercontent.com/ScientiaMesh/agent-kit/main/install.sh | sh
```

## Platform Manifest

Agents should read this manifest when selecting a binary programmatically:

```text
https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/smesh-latest.json
```

## Direct Downloads

```text
https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/smesh-linux-x64
https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/smesh-macos-arm64
https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/smesh-windows-x64.exe
```

The stable `download.scientiamesh.app` URLs may mirror these GitHub release
assets, but the GitHub release is the canonical public distribution source.

## Verify Checksums

```bash
curl -fsSLO https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/smesh-linux-x64
curl -fsSLO https://github.com/ScientiaMesh/agent-kit/releases/download/smesh-latest/smesh-linux-x64.sha256
sha256sum -c smesh-linux-x64.sha256
```
