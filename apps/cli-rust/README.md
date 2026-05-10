# ScientiaMesh Rust CLI

`apps/cli-rust` is the canonical standalone Rust CLI for ScientiaMesh. The Cargo
package remains `smesh-rs` and the installed binary name is `smesh`.

## Local Development

Run from the repository root:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
```

Focused Rust CLI checks:

```bash
cargo fmt -p smesh-rs -- --check
cargo check -p smesh-rs --all-targets
cargo test -p smesh-rs
```

For a local release build:

```bash
cargo build --release -p smesh-rs
./target/release/smesh --help
./target/release/smesh version
```

## Agent Mode

The Rust CLI defaults to JSON when stdout is not a TTY or when
`SMESH_AGENT_MODE=1` is set. Use `--output human`, `--output json`,
`--output ndjson`, or `--json` to override detection for a single command.

See [docs/agent-usage.md](docs/agent-usage.md) for the stdout/stderr contract,
exit codes, write-operation identifiers, and an OpenClaw/Codex capture hook
example.

## Auth Commands

The first Rust auth surface is intentionally token/config based. It is safe for
agents because status output never prints bearer or refresh tokens.

```bash
smesh --json auth status
smesh --output ndjson auth status
smesh --json auth login --access-token "$SMESH_TOKEN" --mesh-id "<mesh-id>"
smesh --json auth logout
```

JSON and NDJSON output modes are intended for agent automation and other
machine consumers, so scripts should prefer them over parsing human output.

`auth status` resolves tokens from `--token`, `SMESH_TOKEN`, `SMESH_API_KEY`,
`AUTH0_ACCESS_TOKEN`, then the selected profile config. `auth login
--access-token` writes the selected profile config using file mode `0600` on
Unix-like systems. `auth logout` removes stored tokens from that profile.

Browser and device-code login are not wired yet. Running `smesh auth login`
without `--access-token` exits non-zero and returns a structured JSON/NDJSON
error when machine output is selected. It does not fake successful auth.

## Job And Capture Status

Capture is the primary enqueue flow for agents and humans. Text capture sends a
JSON request, while file capture uploads the original file bytes as multipart
form data for the server-side capture pipeline:

```bash
smesh --json --mesh-id "<mesh-id>" capture text "Pixel/OpenClaw/Symphony stack notes" \
  --instructions "Summarize and tag the stack" \
  --tag stack --tag pixel

smesh --json --mesh-id "<mesh-id>" capture file ./stack.png \
  --instructions "Summarize this image" \
  --tag image --tag openclaw
```

`capture file` detects common MIME types from the file extension and file
signature, with `--mime-type` available as an explicit override. JSON and NDJSON
responses are normalized for agents and include `job_id`, `capture_id`,
`status`, `file_id`, `file_ids`, `source_links`, `links`, and `details` when
available from the API.

Agents can poll queued capture work by job id or capture id:

```bash
smesh --json jobs get "<job-id>"
smesh --json capture status "<capture-id>"
```

These commands call the portal CLI proxy first and fall back to direct backend
API paths. Responses include processing state, errors, file statuses, and
created Source/Memory IDs when the API has recorded them.

## Retrieval Commands

Agents can retrieve mesh context with stable JSON output:

```bash
smesh --json --mesh-id "<mesh-id>" search --top-k 10 --filter Source "Pixel Symphony vision"
smesh --json --mesh-id "<mesh-id>" ask "What is the captured ScientiaMesh vision?"
smesh --json --mesh-id "<mesh-id>" topics query --topic Pixel --topic Symphony --limit 25
smesh --json --mesh-id "<mesh-id>" topics activity --topic ScientiaMesh
```

`search` and `ask` call the portal CLI proxy first and fall back to `/v1`
backend routes. Topic commands call `/api/topics/query` and
`/api/topics/activity`. Retrieval commands require mesh context through
`--mesh-id`, `SMESH_MESH_ID`, or the selected profile config.

Machine output includes `schema_version: 1` and stable top-level fields.
`--output ndjson` adds a `type` field such as `search.results`, `ask.answer`,
`topics.query`, or `topics.activity`. Human output stays concise: searches list
result titles, ask prints the answer, and topic commands print compact counts.

## Binary Naming And Distribution

- Cargo package: `smesh-rs`
- User-facing binary: `smesh`
- Linux artifact: `smesh-linux-x64`
- macOS Apple Silicon artifact: `smesh-macos-arm64`
- macOS Intel artifact: `smesh-macos-x64`
- Windows artifact: `smesh-windows-x64.exe`

The public distribution path is the `smesh distribution` GitHub Actions
workflow, which builds release binaries, writes SHA-256 checksum files, packages
the `scientiamesh-ea` AgentSkill, and publishes latest/versioned manifests.

Stable public URLs are intended to be served from
`https://download.scientiamesh.app`; see [../../docs/downloads.md](../../docs/downloads.md)
for the current manifest and installer contract. The package name stays
`smesh-rs` so the repository can keep the user-facing `smesh` binary stable
while leaving room for existing preview CLI packaging during the transition.
