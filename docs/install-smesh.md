# Install `smesh`

`smesh` is the ScientiaMesh Rust CLI used by agents to read and write durable
memory.

## Auto Installer

```bash
curl -fsSL https://download.scientiamesh.app/install.sh | sh
smesh --version
```

By default, the installer writes to `$HOME/.local/bin/smesh`.

Override the install location:

```bash
SMESH_INSTALL_DIR="$HOME/bin" curl -fsSL https://download.scientiamesh.app/install.sh | sh
```

## Platform Manifest

Agents should read this manifest when selecting a binary programmatically:

```text
https://download.scientiamesh.app/smesh/latest.json
```

## Direct Downloads

```text
https://download.scientiamesh.app/smesh/latest/linux-x64
https://download.scientiamesh.app/smesh/latest/macos-arm64
https://download.scientiamesh.app/smesh/latest/macos-x64
https://download.scientiamesh.app/smesh/latest/windows-x64.exe
```

Only Linux x64 is currently mirrored. macOS and Windows binaries require the
cross-platform release workflow to publish assets.

## Verify Checksums

```bash
curl -fsSLO https://download.scientiamesh.app/smesh/latest/linux-x64
curl -fsSLO https://download.scientiamesh.app/smesh/latest/linux-x64.sha256
sha256sum -c linux-x64.sha256
```
