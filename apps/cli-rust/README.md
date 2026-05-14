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

The Rust CLI supports browser-based OAuth for humans and token/config auth for
agents. Auth status output never prints bearer or refresh tokens.

```bash
smesh --json auth status
smesh --output ndjson auth status
smesh auth login
smesh auth login --no-open-browser
smesh --json auth login --access-token "$SMESH_TOKEN" --mesh-id "<mesh-id>"
smesh --json auth logout
```

JSON and NDJSON output modes are intended for agent automation and other
machine consumers, so scripts should prefer them over parsing human output.

`smesh auth login` starts an OAuth Authorization Code + PKCE login. The CLI
binds a loopback callback listener on `127.0.0.1` with an ephemeral port, opens
the system browser to the Auth0 `/authorize` URL, validates the callback
`state`, exchanges the authorization `code` plus the PKCE verifier at
`/oauth/token`, and writes the selected profile config using file mode `0600`
on Unix-like systems. The default callback timeout is 300 seconds; override it
with `--callback-timeout-seconds <seconds>`.

Use `--no-open-browser` for remote shells and headless terminals. The CLI prints
the login URL to stderr, waits for the same localhost callback, and keeps stdout
reserved for the final human/JSON/NDJSON result. The browser must be able to
reach the printed `127.0.0.1:<port>` callback on the machine running `smesh`;
when that is not practical, use the token fallback below.

The existing non-interactive path remains supported:

```bash
smesh --json auth login --access-token "$SMESH_TOKEN" \
  --refresh-token "$SMESH_REFRESH_TOKEN" \
  --expires-at 4102444800
```

`auth status` resolves tokens from `--token`, `SMESH_TOKEN`, `SMESH_API_KEY`,
`AUTH0_ACCESS_TOKEN`, then the selected profile config. `auth logout` removes
stored tokens from that profile.

Auth0 operational setup: configure the CLI application as a public/native
client that permits Authorization Code with PKCE and loopback callback redirects
matching `http://127.0.0.1:<ephemeral-port>/callback`. OAuth for native apps
expects authorization servers to allow any port for loopback IP redirect URIs;
if a tenant enforces exact callback URLs, use the concrete URL printed by
`--no-open-browser` for testing or provision a CLI-specific app that supports
loopback redirects.

## Portable Agent Commands

Agents can save and restore portable workspace context through the mesh-backed
agent registry:

```bash
smesh --json --mesh-id "<mesh-id>" agent save Pixel
smesh --json --mesh-id "<mesh-id>" agent init Pixel
smesh --json --mesh-id "<mesh-id>" agent init Pixel --override
```

`agent save` scans a conservative Markdown allowlist from the current
workspace, including `SOUL.md`, `AGENTS.md`, `CLAUDE.md`, `MEMORY.md`,
`USER.md`, `TOOLS.md`, and selected skill/reference Markdown directories. It
writes a generated `.agent-pixel.md` index locally and stores a versioned JSON
manifest in `/api/cli/agent/set`, preserving each artifact path, kind, SHA-256
content hash, timestamp, workspace path, and host identity when available.

`agent init` fetches the latest portable manifest for the named agent, creates
the agent registry entry if it does not exist yet, writes `.agent-pixel.md`, and
restores mesh-stored Markdown artifacts. Existing local files are preserved by
default. `agent init --override` is the explicit destructive mode where
mesh-stored artifacts replace existing local files. Unsafe artifact paths such
as absolute paths, backslash paths, and parent traversal are rejected before any
restore writes occur.

## Mesh Commands

Authenticated users can list the meshes available to the current token/profile:

```bash
smesh mesh list
smesh --json mesh list
```

`mesh list` calls the portal CLI proxy at `/api/cli/meshes` using the same token
resolution order as `auth status`. JSON output includes normalized mesh fields
for `id`, `name`, `type`, `my_role`, `role`, `member_count`, `created_at`,
`description`, and `is_conversation_mesh`. Missing or expired auth fails before
the network request when local config makes that clear.

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
