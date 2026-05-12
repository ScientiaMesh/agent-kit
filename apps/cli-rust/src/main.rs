use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::error::ErrorKind;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use smesh_rs::{
    agent_mode_enabled, output_mode_from_raw_args, run, Cli, CliError, OutputMode, DEFAULT_API_URL,
    DEFAULT_CONFIG_PATH,
};

const MESH_LIST_SCHEMA_VERSION: u8 = 1;
const USER_AGENT: &str = concat!("smesh-rs/", env!("CARGO_PKG_VERSION"));
const MESH_HELP: &str = "Manage meshes.\n\nUsage: smesh mesh <COMMAND>\n\nCommands:\n  list    List meshes available to the authenticated user\n\nOptions:\n  -h, --help    Print help\n";
const MESH_LIST_HELP: &str = "List meshes available to the authenticated user.\n\nUsage: smesh mesh list\n\nOptions:\n  -h, --help    Print help\n";

fn main() {
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let output_mode = output_mode_from_raw_args(raw_args.iter(), agent_mode_enabled());

    match mesh_intercept_from_raw_args(&raw_args, output_mode) {
        Ok(Some(MeshIntercept::Help(help))) => {
            print!("{help}");
            return;
        }
        Ok(Some(MeshIntercept::List(invocation))) => {
            match run_mesh_list(invocation) {
                Ok(output) => print!("{output}"),
                Err(error) => {
                    match output_mode {
                        OutputMode::Human => eprintln!("{}", error.message),
                        OutputMode::Json | OutputMode::Ndjson => {
                            print!("{}", error.render(output_mode))
                        }
                    }
                    std::process::exit(error.exit_code);
                }
            }
            return;
        }
        Ok(None) => {}
        Err(error) => {
            match output_mode {
                OutputMode::Human => eprintln!("{}", error.message),
                OutputMode::Json | OutputMode::Ndjson => print!("{}", error.render(output_mode)),
            }
            std::process::exit(error.exit_code);
        }
    }

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => handle_parse_error(error),
    };
    let output_mode = cli.effective_output();

    match run(cli) {
        Ok(output) => print!("{output}"),
        Err(error) => {
            match output_mode {
                OutputMode::Human => eprintln!("{error}"),
                OutputMode::Json | OutputMode::Ndjson => match error.render(output_mode) {
                    Ok(output) => print!("{output}"),
                    Err(render_error) => eprintln!("{render_error}"),
                },
            }
            std::process::exit(error.exit_code());
        }
    }
}

fn handle_parse_error(error: clap::Error) -> ! {
    let output_mode = output_mode_from_raw_args(std::env::args_os(), agent_mode_enabled());

    if matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) || output_mode == OutputMode::Human
    {
        error.exit();
    }

    let cli_error = CliError::config(strip_ansi_codes(error.to_string().trim()));
    match cli_error.render(output_mode) {
        Ok(output) => print!("{output}"),
        Err(render_error) => eprintln!("{render_error}"),
    }
    std::process::exit(error.exit_code());
}

fn strip_ansi_codes(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if code.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            stripped.push(ch);
        }
    }

    stripped
}

#[derive(Debug)]
enum MeshIntercept {
    Help(&'static str),
    List(MeshListInvocation),
}

#[derive(Debug)]
struct MeshListInvocation {
    output_mode: OutputMode,
    api_url: Option<String>,
    config: Option<PathBuf>,
    profile: String,
    token: Option<String>,
    mesh_id: Option<String>,
}

fn mesh_intercept_from_raw_args(
    args: &[OsString],
    output_mode: OutputMode,
) -> Result<Option<MeshIntercept>, MeshListError> {
    let mut invocation = MeshListInvocation {
        output_mode,
        api_url: None,
        config: None,
        profile: "default".to_string(),
        token: None,
        mesh_id: None,
    };
    let mut positionals = Vec::new();
    let mut help_requested = false;
    let mut index = 1;

    while index < args.len() {
        let arg = arg_to_string(&args[index])?;
        if arg == "--" {
            for value in &args[index + 1..] {
                positionals.push(arg_to_string(value)?);
            }
            break;
        }

        if matches!(arg.as_str(), "-h" | "--help") {
            help_requested = true;
            index += 1;
            continue;
        }

        if arg == "--api-url" {
            invocation.api_url = Some(flag_value(args, index, "--api-url")?);
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--api-url=") {
            invocation.api_url = Some(value.to_string());
            index += 1;
            continue;
        }

        if arg == "--config" {
            invocation.config = Some(PathBuf::from(flag_value(args, index, "--config")?));
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--config=") {
            invocation.config = Some(PathBuf::from(value));
            index += 1;
            continue;
        }

        if arg == "--profile" {
            invocation.profile = flag_value(args, index, "--profile")?;
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--profile=") {
            invocation.profile = value.to_string();
            index += 1;
            continue;
        }

        if arg == "--token" {
            invocation.token = Some(flag_value(args, index, "--token")?);
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--token=") {
            invocation.token = Some(value.to_string());
            index += 1;
            continue;
        }

        if arg == "--mesh-id" {
            invocation.mesh_id = Some(flag_value(args, index, "--mesh-id")?);
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--mesh-id=") {
            invocation.mesh_id = Some(value.to_string());
            index += 1;
            continue;
        }

        if arg == "--output" {
            let _ = flag_value(args, index, "--output")?;
            index += 2;
            continue;
        }
        if arg.starts_with("--output=")
            || matches!(
                arg.as_str(),
                "--json" | "--no-color" | "--quiet" | "--verbose"
            )
        {
            index += 1;
            continue;
        }

        if arg.starts_with('-') {
            index += 1;
            continue;
        }

        positionals.push(arg);
        index += 1;
    }

    if positionals.first().map(String::as_str) != Some("mesh") {
        return Ok(None);
    }

    if help_requested && positionals.len() <= 1 {
        return Ok(Some(MeshIntercept::Help(MESH_HELP)));
    }
    if help_requested && positionals.get(1).map(String::as_str) == Some("list") {
        return Ok(Some(MeshIntercept::Help(MESH_LIST_HELP)));
    }

    match positionals.as_slice() {
        [mesh] if mesh == "mesh" => Err(MeshListError::config(
            "Missing mesh subcommand. Usage: smesh mesh list",
        )),
        [mesh, list] if mesh == "mesh" && list == "list" => {
            Ok(Some(MeshIntercept::List(invocation)))
        }
        [mesh, command, ..] if mesh == "mesh" && command != "list" => Err(MeshListError::config(
            format!("Unrecognized mesh subcommand `{command}`. Supported subcommand: list"),
        )),
        [mesh, list, extra, ..] if mesh == "mesh" && list == "list" => Err(MeshListError::config(
            format!("Unexpected argument `{extra}` for `smesh mesh list`"),
        )),
        _ => Ok(None),
    }
}

fn arg_to_string(arg: &OsStr) -> Result<String, MeshListError> {
    arg.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| MeshListError::config("Command arguments must be valid UTF-8."))
}

fn flag_value(args: &[OsString], index: usize, flag: &str) -> Result<String, MeshListError> {
    args.get(index + 1)
        .ok_or_else(|| MeshListError::config(format!("Missing value for {flag}.")))
        .and_then(|value| arg_to_string(value.as_os_str()))
}

fn run_mesh_list(invocation: MeshListInvocation) -> Result<String, MeshListError> {
    let config_path = invocation
        .config
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));
    let config = MeshListConfigFile::load(&expand_tilde(&config_path))?;
    let profile_name = if invocation.profile == "default" && !config.active_profile.is_empty() {
        config.active_profile.as_str()
    } else {
        invocation.profile.as_str()
    };
    let profile = config.profiles.get(profile_name);
    let api_url = resolve_mesh_list_api_url(&invocation, profile);
    let mesh_id = resolve_mesh_list_mesh_id(&invocation, profile);
    let token = resolve_mesh_list_token(&invocation, profile)?;
    let output = fetch_mesh_list(&api_url, mesh_id.as_deref(), &token.access_token)?;

    match invocation.output_mode {
        OutputMode::Human => Ok(render_mesh_list_human(&output)),
        OutputMode::Json => Ok(format!("{}\n", serde_json::to_string(&output)?)),
        OutputMode::Ndjson => {
            let event = MeshListEvent {
                event_type: "mesh.list",
                output,
            };
            Ok(format!("{}\n", serde_json::to_string(&event)?))
        }
    }
}

fn resolve_mesh_list_api_url(
    invocation: &MeshListInvocation,
    profile: Option<&MeshListProfileConfig>,
) -> String {
    invocation
        .api_url
        .clone()
        .or_else(|| env_first(&["SMESH_API_URL"]))
        .or_else(|| profile.and_then(|profile| profile.api_url.clone()))
        .unwrap_or_else(|| DEFAULT_API_URL.to_string())
}

fn resolve_mesh_list_mesh_id(
    invocation: &MeshListInvocation,
    profile: Option<&MeshListProfileConfig>,
) -> Option<String> {
    invocation
        .mesh_id
        .clone()
        .or_else(|| env_first(&["SMESH_MESH_ID"]))
        .or_else(|| profile.and_then(|profile| profile.mesh_id.clone()))
}

fn resolve_mesh_list_token(
    invocation: &MeshListInvocation,
    profile: Option<&MeshListProfileConfig>,
) -> Result<MeshListToken, MeshListError> {
    if let Some(token) = invocation
        .token
        .clone()
        .filter(|token| !token.trim().is_empty())
        .or_else(|| env_first(&["SMESH_TOKEN"]))
        .or_else(|| env_first(&["SMESH_API_KEY", "AUTH0_ACCESS_TOKEN"]))
    {
        return Ok(MeshListToken {
            access_token: token,
        });
    }

    let auth = profile.and_then(|profile| profile.auth.as_ref());
    if let Some(token) = auth
        .and_then(|auth| auth.access_token.clone())
        .filter(|token| !token.trim().is_empty())
    {
        if auth
            .and_then(|auth| auth.expires_at)
            .is_some_and(|expiry| expiry <= now_unix_seconds())
        {
            return Err(MeshListError::auth(
                "Access token is present but expired. Run `smesh auth login --access-token <token>` or set SMESH_TOKEN.",
            ));
        }
        return Ok(MeshListToken {
            access_token: token,
        });
    }

    Err(MeshListError::auth(
        "Not authenticated. Use `smesh auth login --access-token <token>` or set SMESH_TOKEN.",
    ))
}

fn fetch_mesh_list(
    api_url: &str,
    mesh_id: Option<&str>,
    access_token: &str,
) -> Result<MeshListOutput, MeshListError> {
    let url = format!("{}/api/cli/meshes", api_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| MeshListError::network(format!("Failed to build HTTP client: {error}")))?;
    let mut request = client.get(&url).bearer_auth(access_token);
    if let Some(mesh_id) = mesh_id.filter(|mesh_id| !mesh_id.trim().is_empty()) {
        request = request.header("X-Mesh-Context", mesh_id);
    }

    let response = request
        .send()
        .map_err(|error| MeshListError::network(format!("Mesh list request failed: {error}")))?;
    let status = response.status();
    let body = response.text().map_err(|error| {
        MeshListError::network(format!("Failed to read mesh list response: {error}"))
    })?;

    if !status.is_success() {
        if matches!(status.as_u16(), 401 | 403) {
            return Err(MeshListError::auth(format!(
                "Authentication failed or token expired while listing meshes (HTTP {status}). Run `smesh auth login --access-token <token>` or set SMESH_TOKEN."
            )));
        }
        return Err(MeshListError::network(format!(
            "Mesh list request failed with HTTP {status}: {}",
            body_snippet(&body)
        )));
    }

    let value: Value = serde_json::from_str(&body).map_err(|error| {
        MeshListError::network(format!("Mesh list response was not valid JSON: {error}"))
    })?;
    normalize_mesh_list_response(&value)
}

fn normalize_mesh_list_response(value: &Value) -> Result<MeshListOutput, MeshListError> {
    let meshes = mesh_array(value).ok_or_else(|| {
        MeshListError::network("Mesh list response did not include a meshes array.")
    })?;
    let meshes = meshes.iter().filter_map(normalize_mesh_item).collect();

    Ok(MeshListOutput {
        schema_version: MESH_LIST_SCHEMA_VERSION,
        meshes,
    })
}

fn mesh_array(value: &Value) -> Option<&Vec<Value>> {
    value
        .as_array()
        .or_else(|| value.get("meshes").and_then(Value::as_array))
        .or_else(|| value.pointer("/data/meshes").and_then(Value::as_array))
        .or_else(|| value.get("items").and_then(Value::as_array))
        .or_else(|| value.pointer("/data/items").and_then(Value::as_array))
        .or_else(|| value.get("data").and_then(Value::as_array))
}

fn normalize_mesh_item(value: &Value) -> Option<MeshListMesh> {
    let object = value.as_object()?;
    let mesh_type = string_field(object, &["type", "mesh_type", "meshType"]);
    let is_conversation_mesh = bool_field(
        object,
        &[
            "is_conversation_mesh",
            "isConversationMesh",
            "conversation_mesh",
            "conversationMesh",
        ],
    )
    .or_else(|| {
        mesh_type.as_deref().map(|mesh_type| {
            matches!(
                mesh_type.to_ascii_lowercase().as_str(),
                "conversation" | "conversation_mesh" | "conversation-mesh"
            )
        })
    });
    let my_role = string_field(object, &["my_role", "myRole", "role"]);
    let role = string_field(object, &["role", "my_role", "myRole"]);

    Some(MeshListMesh {
        id: string_field(object, &["id", "mesh_id", "meshId"]),
        name: string_field(object, &["name", "title"]),
        mesh_type,
        my_role,
        role,
        member_count: u64_field(object, &["member_count", "memberCount", "members_count"]),
        created_at: string_field(object, &["created_at", "createdAt"]),
        description: string_field(object, &["description"]),
        is_conversation_mesh,
    })
}

fn string_field(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        match object.get(*key) {
            Some(Value::String(value)) if !value.trim().is_empty() => return Some(value.clone()),
            Some(Value::Number(value)) => return Some(value.to_string()),
            Some(Value::Bool(value)) => return Some(value.to_string()),
            _ => {}
        }
    }
    None
}

fn u64_field(object: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
    for key in keys {
        match object.get(*key) {
            Some(Value::Number(value)) => {
                if let Some(value) = value.as_u64() {
                    return Some(value);
                }
            }
            Some(Value::String(value)) => {
                if let Ok(value) = value.parse() {
                    return Some(value);
                }
            }
            _ => {}
        }
    }
    None
}

fn bool_field(object: &Map<String, Value>, keys: &[&str]) -> Option<bool> {
    for key in keys {
        match object.get(*key) {
            Some(Value::Bool(value)) => return Some(*value),
            Some(Value::String(value)) => match value.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => return Some(true),
                "false" | "0" | "no" | "off" => return Some(false),
                _ => {}
            },
            _ => {}
        }
    }
    None
}

fn render_mesh_list_human(output: &MeshListOutput) -> String {
    if output.meshes.is_empty() {
        return "No meshes found.\n".to_string();
    }

    let mut rendered = format!(
        "{} mesh{}\n",
        output.meshes.len(),
        if output.meshes.len() == 1 { "" } else { "es" }
    );
    for mesh in &output.meshes {
        let name = mesh
            .name
            .as_deref()
            .or(mesh.id.as_deref())
            .unwrap_or("Unnamed mesh");
        rendered.push_str(&format!("- {name}"));
        if let Some(id) = mesh.id.as_deref() {
            rendered.push_str(&format!(" ({id})"));
        }
        rendered.push('\n');
        rendered.push_str(&format!(
            "  Role: {} | Members: {} | Type: {} | Created: {}\n",
            display_field(mesh.my_role.as_deref().or(mesh.role.as_deref())),
            mesh.member_count
                .map(|count| count.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            display_field(mesh.mesh_type.as_deref()),
            display_field(mesh.created_at.as_deref())
        ));
        if let Some(description) = mesh.description.as_deref() {
            rendered.push_str(&format!("  {description}\n"));
        }
        if mesh.is_conversation_mesh == Some(true) {
            rendered.push_str("  Conversation mesh\n");
        }
    }
    rendered
}

fn display_field(value: Option<&str>) -> &str {
    value
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unknown")
}

fn body_snippet(body: &str) -> String {
    let trimmed = body.trim();
    let snippet: String = trimmed.chars().take(200).collect();
    if trimmed.chars().count() > 200 {
        format!("{snippet}...")
    } else if snippet.is_empty() {
        "<empty response body>".to_string()
    } else {
        snippet
    }
}

fn env_first(names: &[&str]) -> Option<String> {
    names
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.trim().is_empty())
}

fn expand_tilde(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    } else if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }

    path.to_path_buf()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MeshListToken {
    access_token: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct MeshListOutput {
    schema_version: u8,
    meshes: Vec<MeshListMesh>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct MeshListEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    #[serde(flatten)]
    output: MeshListOutput,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct MeshListMesh {
    id: Option<String>,
    name: Option<String>,
    #[serde(rename = "type")]
    mesh_type: Option<String>,
    my_role: Option<String>,
    role: Option<String>,
    member_count: Option<u64>,
    created_at: Option<String>,
    description: Option<String>,
    is_conversation_mesh: Option<bool>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
struct MeshListConfigFile {
    #[serde(default)]
    active_profile: String,
    #[serde(default)]
    profiles: BTreeMap<String, MeshListProfileConfig>,
}

impl MeshListConfigFile {
    fn load(path: &Path) -> Result<Self, MeshListError> {
        match fs::read_to_string(path) {
            Ok(contents) => serde_json::from_str(&contents).map_err(|error| {
                MeshListError::config(format!(
                    "Invalid ScientiaMesh config at {}: {error}",
                    path.display()
                ))
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(MeshListError::config(format!(
                "Failed to read ScientiaMesh config at {}: {error}",
                path.display()
            ))),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
struct MeshListProfileConfig {
    api_url: Option<String>,
    mesh_id: Option<String>,
    auth: Option<MeshListAuthConfig>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
struct MeshListAuthConfig {
    access_token: Option<String>,
    expires_at: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MeshListError {
    message: String,
    exit_code: i32,
}

impl MeshListError {
    fn config(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 2,
        }
    }

    fn auth(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 4,
        }
    }

    fn network(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 5,
        }
    }

    fn render(&self, output_mode: OutputMode) -> String {
        let mut value = json!({
            "error": self.message,
            "status": Value::Null,
            "details": Value::Null,
        });
        if output_mode == OutputMode::Ndjson {
            value["type"] = Value::String("error".to_string());
        }
        format!(
            "{}\n",
            serde_json::to_string(&value).unwrap_or_else(|_| {
                "{\"error\":\"failed to render error\",\"status\":null,\"details\":null}"
                    .to_string()
            })
        )
    }
}

impl From<serde_json::Error> for MeshListError {
    fn from(error: serde_json::Error) -> Self {
        Self::network(format!("Failed to serialize mesh list output: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mesh_list_intercept_with_global_flags() {
        let intercept = mesh_intercept_from_raw_args(
            &args(&[
                "smesh",
                "--json",
                "--api-url",
                "http://localhost:8000",
                "--config",
                "/tmp/smesh-config.json",
                "--token",
                "token-test",
                "mesh",
                "list",
            ]),
            OutputMode::Json,
        )
        .expect("valid mesh list intercept")
        .expect("mesh list command");

        let MeshIntercept::List(invocation) = intercept else {
            panic!("expected mesh list invocation");
        };
        assert_eq!(invocation.output_mode, OutputMode::Json);
        assert_eq!(invocation.api_url.as_deref(), Some("http://localhost:8000"));
        assert_eq!(
            invocation.config.as_deref(),
            Some(Path::new("/tmp/smesh-config.json"))
        );
        assert_eq!(invocation.token.as_deref(), Some("token-test"));
    }

    #[test]
    fn normalizes_mesh_list_response_shape() {
        let response = json!({
            "data": {
                "meshes": [
                    {
                        "id": "mesh-1",
                        "name": "Personal",
                        "type": "personal",
                        "my_role": "owner",
                        "member_count": 2,
                        "created_at": "2026-05-09T18:25:32Z",
                        "description": "Private working mesh",
                        "is_conversation_mesh": false
                    },
                    {
                        "meshId": "mesh-2",
                        "title": "AI Talk",
                        "meshType": "conversation",
                        "role": "member",
                        "memberCount": "5",
                        "createdAt": "2026-05-10T03:26:08Z",
                        "isConversationMesh": true
                    }
                ]
            }
        });

        let output = normalize_mesh_list_response(&response).expect("normalized mesh list");
        let value = serde_json::to_value(output).expect("serialized mesh list");
        assert_eq!(
            value,
            json!({
                "schema_version": 1,
                "meshes": [
                    {
                        "id": "mesh-1",
                        "name": "Personal",
                        "type": "personal",
                        "my_role": "owner",
                        "role": "owner",
                        "member_count": 2,
                        "created_at": "2026-05-09T18:25:32Z",
                        "description": "Private working mesh",
                        "is_conversation_mesh": false
                    },
                    {
                        "id": "mesh-2",
                        "name": "AI Talk",
                        "type": "conversation",
                        "my_role": "member",
                        "role": "member",
                        "member_count": 5,
                        "created_at": "2026-05-10T03:26:08Z",
                        "description": null,
                        "is_conversation_mesh": true
                    }
                ]
            })
        );
    }

    #[test]
    fn renders_mesh_list_human_summary() {
        let output = MeshListOutput {
            schema_version: 1,
            meshes: vec![MeshListMesh {
                id: Some("mesh-1".to_string()),
                name: Some("Personal".to_string()),
                mesh_type: Some("personal".to_string()),
                my_role: Some("owner".to_string()),
                role: Some("owner".to_string()),
                member_count: Some(1),
                created_at: Some("2026-05-09T18:25:32Z".to_string()),
                description: Some("Private working mesh".to_string()),
                is_conversation_mesh: Some(false),
            }],
        };

        let rendered = render_mesh_list_human(&output);
        assert!(rendered.contains("1 mesh"));
        assert!(rendered.contains("- Personal (mesh-1)"));
        assert!(rendered.contains("Role: owner"));
        assert!(rendered.contains("Members: 1"));
        assert!(rendered.contains("Private working mesh"));
    }

    #[test]
    fn renders_mesh_auth_error_as_json() {
        let rendered = MeshListError::auth("Not authenticated").render(OutputMode::Json);
        let value: Value = serde_json::from_str(rendered.trim()).expect("json error");
        assert_eq!(value["error"], "Not authenticated");
        assert_eq!(value["status"], Value::Null);
        assert_eq!(value["details"], Value::Null);
    }

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
}
