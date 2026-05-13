use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{CommandFactory, Parser};
use serde_json::json;
use smesh_rs::{
    render_version, AgentCommands, AssertionCommands, AuthCommands, BriefCommands,
    CalendarCommands, CalendarEventCommands, CaptureCommands, Cli, Commands, ContactCommands,
    ContactOrgCommands, ContactPeopleCommands, ContactRelationshipCommands, JobCommands,
    OutputMode, PreferenceCommands, ProjectCommands, ReminderCommands, SourceLinkCommands,
    TaskAssertionCommands, TaskCommands, TopicCommands, TopicMatch, TopicSort,
};

#[test]
fn parses_version_command() {
    let cli = Cli::try_parse_from([
        "smesh",
        "--api-url",
        "http://localhost:8000",
        "--mesh-id",
        "3eee6a12-fcd7-4003-8e1a-3a864981c5bc",
        "version",
    ])
    .expect("valid version command");

    assert!(matches!(cli.command, Commands::Version));
    assert_eq!(cli.api_url, "http://localhost:8000");
    assert_eq!(
        cli.mesh_id.as_deref(),
        Some("3eee6a12-fcd7-4003-8e1a-3a864981c5bc")
    );
    assert_eq!(cli.effective_output_for(false), OutputMode::Human);
}

#[test]
fn default_output_selects_json_in_agent_mode() {
    let cli = Cli::try_parse_from(["smesh", "version"]).expect("valid version command");

    assert_eq!(cli.effective_output_for(true), OutputMode::Json);
}

#[test]
fn output_human_overrides_agent_mode() {
    let cli = Cli::try_parse_from(["smesh", "--output", "human", "version"])
        .expect("valid human output command");

    assert_eq!(cli.effective_output_for(true), OutputMode::Human);
}

#[test]
fn json_flag_aliases_output_json() {
    let cli =
        Cli::try_parse_from(["smesh", "--json", "version"]).expect("valid JSON version command");

    assert_eq!(cli.effective_output(), OutputMode::Json);
}

#[test]
fn output_json_selects_json() {
    let cli =
        Cli::try_parse_from(["smesh", "--output", "json", "version"]).expect("valid output mode");

    assert_eq!(cli.effective_output(), OutputMode::Json);
}

#[test]
fn help_lists_foundation_command_and_global_flags() {
    let mut command = Cli::command();
    let help = command.render_long_help().to_string();

    assert!(help.contains("version"));
    assert!(help.contains("auth"));
    assert!(help.contains("agent"));
    assert!(help.contains("jobs"));
    assert!(help.contains("capture"));
    assert!(help.contains("search"));
    assert!(help.contains("ask"));
    assert!(help.contains("topics"));
    assert!(help.contains("projects"));
    assert!(help.contains("assertions"));
    assert!(help.contains("tasks"));
    assert!(help.contains("reminders"));
    assert!(help.contains("contacts"));
    assert!(help.contains("preferences"));
    assert!(help.contains("briefs"));
    assert!(help.contains("calendar"));
    assert!(help.contains("source-links"));
    assert!(help.contains("--api-url"));
    assert!(help.contains("--mesh-id"));
    assert!(help.contains("--json"));
}

#[test]
fn parses_agent_init_and_save_contract() {
    let init = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "agent",
        "init",
        "Pixel",
        "--override",
    ])
    .expect("valid agent init command");

    assert!(matches!(
        init.command,
        Commands::Agent {
            command: AgentCommands::Init(ref args)
        } if args.name == "Pixel" && args.override_existing
    ));

    let save = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "agent",
        "save",
        "Pixel",
    ])
    .expect("valid agent save command");

    assert!(matches!(
        save.command,
        Commands::Agent {
            command: AgentCommands::Save(ref args)
        } if args.name == "Pixel"
    ));
}

#[test]
fn agent_init_first_time_creates_agent_and_index() {
    let config = temp_config_path("agent-init-first");
    let workspace = temp_data_path("agent-init-first-workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    let server = MockServer::start(vec![
        MockResponse::json(404, r#"{"detail":"agent not found"}"#),
        MockResponse::json(
            200,
            r#"{"key":"Pixel","version":"agent-v1","format":"json","content":"","synapse_task_id":"task-agent"}"#,
        ),
    ]);

    let output = smesh_command()
        .current_dir(&workspace)
        .arg("--json")
        .arg("--api-url")
        .arg(server.url())
        .arg("--config")
        .arg(&config)
        .arg("--token")
        .arg("token-test")
        .arg("--mesh-id")
        .arg("mesh-test")
        .arg("agent")
        .arg("init")
        .arg("Pixel")
        .output()
        .expect("run agent init");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(value["action"], "agent.init");
    assert_eq!(value["agent_name"], "Pixel");
    assert_eq!(value["created_agent"], true);
    assert_eq!(value["agent_version"], "agent-v1");
    assert_eq!(value["artifacts_total"], 0);
    assert!(workspace.join(".agent-pixel.md").is_file());

    let requests = server.join();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(
        requests[0].path,
        "/api/cli/agent/get?agent_id=Pixel&mesh_id=mesh-test"
    );
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/cli/agent/set");
    assert_eq!(
        requests[1].authorization.as_deref(),
        Some("Bearer token-test")
    );
    assert_eq!(requests[1].mesh_id.as_deref(), Some("mesh-test"));
    let payload: serde_json::Value =
        serde_json::from_str(&requests[1].body).expect("agent set payload");
    assert_eq!(payload["agent_id"], "Pixel");
    assert_eq!(payload["mesh_id"], "mesh-test");
    assert_eq!(payload["format"], "json");
    let content = payload["content"].as_str().expect("manifest content");
    let manifest: serde_json::Value = serde_json::from_str(content).expect("manifest JSON");
    assert_eq!(manifest["agent_name"], "Pixel");
    assert_eq!(
        manifest["artifacts"].as_array().expect("artifacts").len(),
        0
    );
}

#[test]
fn agent_save_then_init_restores_saved_markdown_artifacts() {
    let save_config = temp_config_path("agent-save");
    let save_workspace = temp_data_path("agent-save-workspace");
    fs::create_dir_all(&save_workspace).expect("create save workspace");
    fs::write(save_workspace.join("SOUL.md"), "# Soul\nSaved identity\n").expect("write soul");
    fs::write(
        save_workspace.join("AGENTS.md"),
        "# Agents\nSaved operating doc\n",
    )
    .expect("write agents");
    let save_server = MockServer::start(vec![
        MockResponse::json(
            200,
            r#"{"key":"Pixel","version":"agent-save-v1","format":"json","content":"","synapse_task_id":"task-save"}"#,
        ),
        MockResponse::json(
            200,
            r#"{"key":".agent-pixel.md","version":"context-index-v1","format":"md","content":"","agent_id":"Pixel","synapse_task_id":"task-index"}"#,
        ),
        MockResponse::json(
            200,
            r#"{"key":"AGENTS.md","version":"context-agents-v1","format":"md","content":"","agent_id":"Pixel","synapse_task_id":"task-agents"}"#,
        ),
        MockResponse::json(
            200,
            r#"{"key":"SOUL.md","version":"context-soul-v1","format":"md","content":"","agent_id":"Pixel","synapse_task_id":"task-soul"}"#,
        ),
    ]);

    let save_output = smesh_command()
        .current_dir(&save_workspace)
        .arg("--json")
        .arg("--api-url")
        .arg(save_server.url())
        .arg("--config")
        .arg(&save_config)
        .arg("--token")
        .arg("token-test")
        .arg("--mesh-id")
        .arg("mesh-test")
        .arg("agent")
        .arg("save")
        .arg("Pixel")
        .output()
        .expect("run agent save");

    assert!(save_output.status.success());
    let save_stdout = String::from_utf8(save_output.stdout).expect("utf8 stdout");
    let save_value: serde_json::Value =
        serde_json::from_str(save_stdout.trim()).expect("valid JSON");
    assert_eq!(save_value["artifacts_saved"], 2);
    assert!(save_workspace.join(".agent-pixel.md").is_file());

    let save_requests = save_server.join();
    assert_eq!(save_requests.len(), 4);
    assert_eq!(save_requests[0].path, "/api/cli/agent/set");
    let save_payload: serde_json::Value =
        serde_json::from_str(&save_requests[0].body).expect("agent set payload");
    let manifest_content = save_payload["content"]
        .as_str()
        .expect("portable manifest content")
        .to_string();
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_content).expect("manifest JSON");
    let artifact_paths = manifest["artifacts"]
        .as_array()
        .expect("artifacts")
        .iter()
        .map(|artifact| artifact["path"].as_str().expect("path"))
        .collect::<Vec<_>>();
    assert_eq!(artifact_paths, vec!["AGENTS.md", "SOUL.md"]);
    assert_eq!(
        manifest["artifacts"][0]["source"]["workspace_path"],
        save_workspace.display().to_string()
    );
    assert!(
        manifest["artifacts"][0]["sha256"]
            .as_str()
            .expect("sha")
            .len()
            >= 64
    );
    assert_eq!(save_requests[1].path, "/api/cli/context/set");
    let index_payload: serde_json::Value =
        serde_json::from_str(&save_requests[1].body).expect("index context payload");
    assert_eq!(index_payload["agent_id"], "Pixel");
    assert_eq!(index_payload["key"], ".agent-pixel.md");
    assert_eq!(index_payload["mesh_id"], "mesh-test");
    assert_eq!(index_payload["format"], "md");
    assert_eq!(save_requests[2].path, "/api/cli/context/set");
    let agents_payload: serde_json::Value =
        serde_json::from_str(&save_requests[2].body).expect("agents context payload");
    assert_eq!(agents_payload["agent_id"], "Pixel");
    assert_eq!(agents_payload["key"], "AGENTS.md");
    assert_eq!(save_requests[3].path, "/api/cli/context/set");
    let soul_payload: serde_json::Value =
        serde_json::from_str(&save_requests[3].body).expect("soul context payload");
    assert_eq!(soul_payload["agent_id"], "Pixel");
    assert_eq!(soul_payload["key"], "SOUL.md");

    let init_config = temp_config_path("agent-init-after-save");
    let init_workspace = temp_data_path("agent-init-after-save-workspace");
    fs::create_dir_all(&init_workspace).expect("create init workspace");
    let get_body = json!({
        "key": "Pixel",
        "version": "agent-save-v1",
        "format": "json",
        "content": manifest_content,
    })
    .to_string();
    let init_server = MockServer::start(vec![MockResponse::json(200, &get_body)]);

    let init_output = smesh_command()
        .current_dir(&init_workspace)
        .arg("--json")
        .arg("--api-url")
        .arg(init_server.url())
        .arg("--config")
        .arg(&init_config)
        .arg("--token")
        .arg("token-test")
        .arg("--mesh-id")
        .arg("mesh-test")
        .arg("agent")
        .arg("init")
        .arg("Pixel")
        .output()
        .expect("run agent init");

    assert!(init_output.status.success());
    assert_eq!(
        fs::read_to_string(init_workspace.join("SOUL.md")).expect("restored soul"),
        "# Soul\nSaved identity\n"
    );
    assert_eq!(
        fs::read_to_string(init_workspace.join("AGENTS.md")).expect("restored agents"),
        "# Agents\nSaved operating doc\n"
    );
    assert!(init_workspace.join(".agent-pixel.md").is_file());

    let init_requests = init_server.join();
    assert_eq!(init_requests.len(), 1);
    assert_eq!(init_requests[0].method, "GET");
}

#[test]
fn agent_init_without_override_preserves_existing_local_files() {
    let config = temp_config_path("agent-init-preserve");
    let workspace = temp_data_path("agent-init-preserve-workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("SOUL.md"), "local identity\n").expect("write local soul");
    let manifest = agent_manifest_with_artifacts(vec![agent_manifest_artifact(
        "SOUL.md",
        "identity",
        "mesh identity\n",
    )]);
    let get_body = json!({
        "key": "Pixel",
        "version": "agent-v1",
        "format": "json",
        "content": manifest,
    })
    .to_string();
    let server = MockServer::start(vec![MockResponse::json(200, &get_body)]);

    let output = smesh_command()
        .current_dir(&workspace)
        .arg("--json")
        .arg("--api-url")
        .arg(server.url())
        .arg("--config")
        .arg(&config)
        .arg("--token")
        .arg("token-test")
        .arg("--mesh-id")
        .arg("mesh-test")
        .arg("agent")
        .arg("init")
        .arg("Pixel")
        .output()
        .expect("run agent init");

    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(workspace.join("SOUL.md")).expect("local soul"),
        "local identity\n"
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert!(value["skipped"]
        .as_array()
        .expect("skipped")
        .iter()
        .any(|item| item["path"] == "SOUL.md"));

    server.join();
}

#[test]
fn agent_init_with_override_replaces_existing_local_files() {
    let config = temp_config_path("agent-init-override");
    let workspace = temp_data_path("agent-init-override-workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("SOUL.md"), "local identity\n").expect("write local soul");
    let manifest = agent_manifest_with_artifacts(vec![agent_manifest_artifact(
        "SOUL.md",
        "identity",
        "mesh identity\n",
    )]);
    let get_body = json!({
        "key": "Pixel",
        "version": "agent-v1",
        "format": "json",
        "content": manifest,
    })
    .to_string();
    let server = MockServer::start(vec![MockResponse::json(200, &get_body)]);

    let output = smesh_command()
        .current_dir(&workspace)
        .arg("--json")
        .arg("--api-url")
        .arg(server.url())
        .arg("--config")
        .arg(&config)
        .arg("--token")
        .arg("token-test")
        .arg("--mesh-id")
        .arg("mesh-test")
        .arg("agent")
        .arg("init")
        .arg("Pixel")
        .arg("--override")
        .output()
        .expect("run agent init override");

    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(workspace.join("SOUL.md")).expect("overwritten soul"),
        "mesh identity\n"
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(value["override_existing"], true);
    assert!(value["restored"]
        .as_array()
        .expect("restored")
        .iter()
        .any(|item| item["path"] == "SOUL.md" && item["status"] == "overwritten"));

    server.join();
}

#[test]
fn agent_init_empty_mesh_state_generates_index_without_artifacts() {
    let config = temp_config_path("agent-init-empty");
    let workspace = temp_data_path("agent-init-empty-workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    let server = MockServer::start(vec![MockResponse::json(
        200,
        r#"{"key":"Pixel","version":"agent-empty","format":"json","content":""}"#,
    )]);

    let output = smesh_command()
        .current_dir(&workspace)
        .arg("--json")
        .arg("--api-url")
        .arg(server.url())
        .arg("--config")
        .arg(&config)
        .arg("--token")
        .arg("token-test")
        .arg("--mesh-id")
        .arg("mesh-test")
        .arg("agent")
        .arg("init")
        .arg("Pixel")
        .output()
        .expect("run agent init empty");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(value["artifacts_total"], 0);
    assert!(workspace.join(".agent-pixel.md").is_file());

    server.join();
}

#[test]
fn agent_init_rejects_unsafe_artifact_paths() {
    let config = temp_config_path("agent-init-unsafe");
    let workspace = temp_data_path("agent-init-unsafe-workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    let manifest = agent_manifest_with_artifacts(vec![agent_manifest_artifact(
        "../SOUL.md",
        "identity",
        "unsafe\n",
    )]);
    let get_body = json!({
        "key": "Pixel",
        "version": "agent-v1",
        "format": "json",
        "content": manifest,
    })
    .to_string();
    let server = MockServer::start(vec![MockResponse::json(200, &get_body)]);

    let output = smesh_command()
        .current_dir(&workspace)
        .arg("--json")
        .arg("--api-url")
        .arg(server.url())
        .arg("--config")
        .arg(&config)
        .arg("--token")
        .arg("token-test")
        .arg("--mesh-id")
        .arg("mesh-test")
        .arg("agent")
        .arg("init")
        .arg("Pixel")
        .output()
        .expect("run unsafe agent init");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON error");
    assert!(value["error"]
        .as_str()
        .expect("error string")
        .contains("Unsafe agent artifact path"));
    assert!(!workspace.join(".agent-pixel.md").exists());
    assert!(!workspace.join("SOUL.md").exists());

    server.join();
}

#[cfg(unix)]
#[test]
fn agent_init_rejects_symlink_parent_escape() {
    use std::os::unix::fs::symlink;

    let config = temp_config_path("agent-init-symlink-parent");
    let workspace = temp_data_path("agent-init-symlink-parent-workspace");
    let outside = temp_data_path("agent-init-symlink-parent-outside");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::create_dir_all(&outside).expect("create outside target");
    symlink(&outside, workspace.join("docs")).expect("create workspace symlink");

    let manifest = agent_manifest_with_artifacts(vec![agent_manifest_artifact(
        "docs/SOUL.md",
        "identity",
        "escaped\n",
    )]);
    let get_body = json!({
        "key": "Pixel",
        "version": "agent-v1",
        "format": "json",
        "content": manifest,
    })
    .to_string();
    let server = MockServer::start(vec![MockResponse::json(200, &get_body)]);

    let output = smesh_command()
        .current_dir(&workspace)
        .arg("--json")
        .arg("--api-url")
        .arg(server.url())
        .arg("--config")
        .arg(&config)
        .arg("--token")
        .arg("token-test")
        .arg("--mesh-id")
        .arg("mesh-test")
        .arg("agent")
        .arg("init")
        .arg("Pixel")
        .arg("--override")
        .output()
        .expect("run symlink escape agent init");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON error");
    assert!(value["error"]
        .as_str()
        .expect("error string")
        .contains("resolved parent escapes the workspace"));
    assert!(!outside.join("SOUL.md").exists());

    server.join();
}

#[test]
fn parses_canonical_project_and_source_link_contract() {
    let project = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "projects",
        "create",
        "ACME",
        "Renewal",
        "--summary",
        "Track the renewal",
        "--tag",
        "customer,renewal",
        "--actor",
        "agent:pixel",
        "--idempotency-key",
        "pixel:project:acme-renewal:create",
    ])
    .expect("valid project command");

    assert!(matches!(
        project.command,
        Commands::Projects {
            command: ProjectCommands::Create(ref args)
        } if args.title == ["ACME", "Renewal"]
            && args.summary.as_deref() == Some("Track the renewal")
            && args.write.actor.as_deref() == Some("agent:pixel")
            && args.write.idempotency_key.as_deref()
                == Some("pixel:project:acme-renewal:create")
    ));

    let source_link = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "source-links",
        "add",
        "--target-type",
        "project",
        "--target-id",
        "project-acme",
        "--source-type",
        "source_node",
        "--source-id",
        "source-meeting",
        "--actor",
        "agent:pixel",
        "--idempotency-key",
        "pixel:source-link:project-acme:source-meeting",
    ])
    .expect("valid source link command");

    assert!(matches!(
        source_link.command,
        Commands::SourceLinks {
            command: SourceLinkCommands::Add(ref args)
        } if args.target_type == "project"
            && args.target_id == "project-acme"
            && args.source_type == "source_node"
            && args.source_id == "source-meeting"
            && args.write.actor.as_deref() == Some("agent:pixel")
    ));
}

#[test]
fn parses_project_assertion_review_contract() {
    let assertion = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "assertions",
        "merge",
        "assertion-acme",
        "--target-project-id",
        "project-acme",
        "--merge-mode",
        "attach_only",
        "--actor",
        "agent:pixel",
        "--idempotency-key",
        "pixel:assertion:acme:merge",
    ])
    .expect("valid assertion command");

    assert!(matches!(
        assertion.command,
        Commands::Assertions {
            command: AssertionCommands::Merge(ref args)
        } if args.id == "assertion-acme"
            && args.target_project_id == "project-acme"
            && args.merge_mode == "attach_only"
            && args.write.actor.as_deref() == Some("agent:pixel")
    ));
}

#[test]
fn parses_task_assertion_review_contract() {
    let assertion = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "task-assertions",
        "confirm",
        "task-assert-acme",
        "--task-id",
        "task-acme",
        "--priority",
        "high",
        "--due-at",
        "2026-05-13T17:00:00Z",
        "--actor",
        "agent:pixel",
        "--idempotency-key",
        "pixel:task-assertion:acme:confirm",
    ])
    .expect("valid task assertion command");

    assert!(matches!(
        assertion.command,
        Commands::TaskAssertions {
            command: TaskAssertionCommands::Confirm(ref args)
        } if args.id == "task-assert-acme"
            && args.overrides.task_id.as_deref() == Some("task-acme")
            && args.overrides.priority.as_deref() == Some("high")
            && args.overrides.due_at.as_deref() == Some("2026-05-13T17:00:00Z")
            && args.write.actor.as_deref() == Some("agent:pixel")
    ));
}

#[test]
fn project_create_json_matches_canonical_golden_envelope() {
    let golden = include_str!("../../../tests/golden/agent_crud/project_create.json");
    let (api_url, requests, handle) = start_json_server(golden);
    let config = temp_config_path("project-create");

    let output = smesh_command()
        .arg("--json")
        .arg("--api-url")
        .arg(&api_url)
        .arg("--config")
        .arg(&config)
        .arg("--token")
        .arg("token-test")
        .arg("--mesh-id")
        .arg("mesh-1")
        .arg("projects")
        .arg("create")
        .arg("ACME")
        .arg("Renewal")
        .arg("--summary")
        .arg("Track the 2026 contract renewal.")
        .arg("--tag")
        .arg("customer,renewal")
        .arg("--source-type")
        .arg("conversation")
        .arg("--source-id")
        .arg("convo-1")
        .arg("--actor")
        .arg("agent:pixel")
        .arg("--idempotency-key")
        .arg("pixel:project:acme:create")
        .output()
        .expect("run project create");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let actual: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let expected: serde_json::Value = serde_json::from_str(golden).expect("golden JSON");
    assert_eq!(actual, expected);

    let request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("captured request");
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/cli/projects");
    assert_eq!(
        request.headers.get("x-mesh-context").map(String::as_str),
        Some("mesh-1")
    );
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
    assert_eq!(body["mesh_id"], "mesh-1");
    assert_eq!(body["title"], "ACME Renewal");
    assert_eq!(body["actor"], json!({"type": "agent", "id": "pixel"}));
    assert_eq!(body["idempotency_key"], "pixel:project:acme:create");

    handle.join().expect("server thread");
}

#[test]
fn parses_assistant_task_create_contract() {
    let cli = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "3eee6a12-fcd7-4003-8e1a-3a864981c5bc",
        "tasks",
        "create",
        "Send",
        "revised",
        "contract",
        "--description",
        "Use May 8 redlines",
        "--project-id",
        "project-acme-renewal",
        "--due-at",
        "2026-05-12T21:00:00Z",
        "--priority",
        "high",
        "--tag",
        "legal,acme",
        "--source-type",
        "source_node",
        "--source-id",
        "source-meeting-2026-05-08",
    ])
    .expect("valid task create command");

    assert!(matches!(
        cli.command,
        Commands::Tasks {
            command: TaskCommands::Create(ref args)
        } if args.title == ["Send", "revised", "contract"]
            && args.description.as_deref() == Some("Use May 8 redlines")
            && args.project_id.as_deref() == Some("project-acme-renewal")
            && args.priority == "high"
            && args.due_at.as_deref() == Some("2026-05-12T21:00:00Z")
            && args.tags == ["legal,acme"]
            && args.source_type.as_deref() == Some("source_node")
            && args.source_id.as_deref() == Some("source-meeting-2026-05-08")
    ));
}

#[test]
fn parses_assistant_reminder_and_preference_commands() {
    let reminder = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "reminders",
        "due-soon",
        "--window",
        "PT12H",
    ])
    .expect("valid reminder command");

    assert!(matches!(
        reminder.command,
        Commands::Reminders {
            command: ReminderCommands::DueSoon(ref args)
        } if args.window == "PT12H"
    ));

    let preference = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "preferences",
        "set",
        "--scope",
        "project:project-acme-renewal",
        "--key",
        "briefs.daily.max_length",
        "--value",
        "short",
        "--surface",
        "cli,mcp",
        "--enforcement",
        "default",
        "--update-rule",
        "confirm_on_conflict",
    ])
    .expect("valid preference command");

    assert!(matches!(
        preference.command,
        Commands::Preferences {
            command: PreferenceCommands::Set(ref args)
        } if args.scope == "project:project-acme-renewal"
            && args.key == "briefs.daily.max_length"
            && args.value.as_deref() == Some("short")
            && args.surface == ["cli,mcp"]
            && args.enforcement.as_deref() == Some("default")
            && args.update_rule == "confirm_on_conflict"
    ));

    let resolve = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "preferences",
        "resolve",
        "--actor",
        "agent:openclaw",
        "--surface",
        "cli",
        "--tool",
        "linear_graphql,git,gh",
        "--action",
        "github.pr.create",
        "--target",
        "project:scientia_dev",
        "--target",
        "linear_issue:SCI-107",
        "--include-evidence",
    ])
    .expect("valid preference resolve command");

    assert!(matches!(
        resolve.command,
        Commands::Preferences {
            command: PreferenceCommands::Resolve(ref args)
        } if args.actor == "agent:openclaw"
            && args.surface == "cli"
            && args.tools == ["linear_graphql,git,gh"]
            && args.action.as_deref() == Some("github.pr.create")
            && args.targets == ["project:scientia_dev", "linear_issue:SCI-107"]
            && args.include_evidence
    ));
}

#[test]
fn parses_assistant_contact_brief_and_calendar_commands() {
    let contact = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "contacts",
        "people",
        "create",
        "--name",
        "Maya Chen",
        "--email",
        "maya@example.com",
    ])
    .expect("valid contact command");

    assert!(matches!(
        contact.command,
        Commands::Contacts {
            command: ContactCommands::People {
                command: ContactPeopleCommands::Create(ref args)
            }
        } if args.name == "Maya Chen" && args.email.as_deref() == Some("maya@example.com")
    ));

    let brief = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "briefs",
        "meeting-prep",
        "cal_evt_123",
        "--refresh",
    ])
    .expect("valid brief command");

    assert!(matches!(
        brief.command,
        Commands::Briefs {
            command: BriefCommands::MeetingPrep(ref args)
        } if args.event_id == "cal_evt_123" && args.refresh
    ));

    let calendar = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "calendar",
        "events",
        "list",
        "--from",
        "2026-05-12T00:00:00Z",
        "--to",
        "2026-05-13T00:00:00Z",
    ])
    .expect("valid calendar command");

    assert!(matches!(
        calendar.command,
        Commands::Calendar {
            command: CalendarCommands::Events {
                command: CalendarEventCommands::List(ref args)
            }
        } if args.from_ts == "2026-05-12T00:00:00Z"
            && args.to_ts == "2026-05-13T00:00:00Z"
    ));
}

#[test]
fn parses_contact_lifecycle_and_relationship_commands() {
    let person_update = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "contacts",
        "people",
        "update",
        "person_123",
        "--email",
        "maya@example.com",
        "--preferred-channel",
        "signal",
    ])
    .expect("valid person update command");

    assert!(matches!(
        person_update.command,
        Commands::Contacts {
            command: ContactCommands::People {
                command: ContactPeopleCommands::Update(ref args)
            }
        } if args.id == "person_123"
            && args.email.as_deref() == Some("maya@example.com")
            && args.preferred_channel.as_deref() == Some("signal")
    ));

    let org_merge = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "contacts",
        "orgs",
        "merge",
        "org_old",
        "--into",
        "org_new",
    ])
    .expect("valid org merge command");

    assert!(matches!(
        org_merge.command,
        Commands::Contacts {
            command: ContactCommands::Orgs {
                command: ContactOrgCommands::Merge(ref args)
            }
        } if args.id == "org_old" && args.into == "org_new"
    ));

    let relationship_add = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "contacts",
        "relationships",
        "add",
        "--from",
        "person_123",
        "--to",
        "project:acme",
        "--type",
        "owner_of",
    ])
    .expect("valid relationship add command");

    assert!(matches!(
        relationship_add.command,
        Commands::Contacts {
            command: ContactCommands::Relationships {
                command: ContactRelationshipCommands::Add(ref args)
            }
        } if args.from_contact_id == "person_123"
            && args.to == "project:acme"
            && args.relationship_type == "owner_of"
    ));
}

#[test]
fn version_json_serializes_stable_shape() {
    let output = render_version(OutputMode::Json).expect("version output");
    let value: serde_json::Value = serde_json::from_str(output.trim()).expect("valid JSON output");

    assert_eq!(
        value,
        json!({
            "binary": "smesh",
            "package": "smesh-rs",
            "version": env!("CARGO_PKG_VERSION")
        })
    );
}

#[test]
fn version_ndjson_serializes_event_shape() {
    let output = render_version(OutputMode::Ndjson).expect("version event");
    let value: serde_json::Value = serde_json::from_str(output.trim()).expect("valid NDJSON event");

    assert_eq!(value["type"], "version");
    assert_eq!(value["binary"], "smesh");
    assert_eq!(value["package"], "smesh-rs");
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn binary_help_succeeds() {
    let output = Command::new(env!("CARGO_BIN_EXE_smesh"))
        .arg("--help")
        .output()
        .expect("run smesh --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Usage: smesh"));
    assert!(stdout.contains("version"));
}

#[test]
fn binary_version_defaults_to_json_for_captured_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_smesh"))
        .arg("version")
        .output()
        .expect("run smesh version");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    assert_eq!(value["binary"], "smesh");
    assert_eq!(value["package"], "smesh-rs");
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn binary_version_human_output_can_be_requested() {
    let output = Command::new(env!("CARGO_BIN_EXE_smesh"))
        .arg("--output")
        .arg("human")
        .arg("version")
        .output()
        .expect("run smesh version");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf8 stdout"),
        format!("smesh {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn binary_parse_error_defaults_to_json_for_captured_output() {
    let output = smesh_command()
        .arg("not-a-command")
        .output()
        .expect("run invalid command");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    assert!(value["error"]
        .as_str()
        .expect("error string")
        .contains("unrecognized subcommand"));
    assert_eq!(value["status"], serde_json::Value::Null);
}

#[test]
fn parses_auth_status_command() {
    let cli = Cli::try_parse_from(["smesh", "--output", "ndjson", "auth", "status"])
        .expect("valid auth status command");

    assert!(matches!(
        cli.command,
        Commands::Auth {
            command: AuthCommands::Status
        }
    ));
    assert_eq!(cli.effective_output(), OutputMode::Ndjson);
}

#[test]
fn parses_jobs_get_command() {
    let cli = Cli::try_parse_from(["smesh", "--json", "jobs", "get", "job-123"])
        .expect("valid jobs get command");

    assert!(matches!(
        cli.command,
        Commands::Jobs {
            command: JobCommands::Get(ref args)
        } if args.id == "job-123"
    ));
    assert_eq!(cli.effective_output(), OutputMode::Json);
}

#[test]
fn parses_capture_status_command() {
    let cli = Cli::try_parse_from([
        "smesh",
        "--output",
        "ndjson",
        "capture",
        "status",
        "capture-123",
    ])
    .expect("valid capture status command");

    assert!(matches!(
        cli.command,
        Commands::Capture {
            command: CaptureCommands::Status(ref args)
        } if args.id == "capture-123"
    ));
    assert_eq!(cli.effective_output(), OutputMode::Ndjson);
}

#[test]
fn parses_capture_text_command_with_instructions_and_tags() {
    let cli = Cli::try_parse_from([
        "smesh",
        "--json",
        "--mesh-id",
        "mesh-test",
        "capture",
        "text",
        "Pixel",
        "OpenClaw",
        "Symphony",
        "--instructions",
        "Summarize the stack",
        "--tag",
        "pixel",
        "--tag",
        "#OpenClaw,pixel",
    ])
    .expect("valid capture text command");

    assert!(matches!(
        cli.command,
        Commands::Capture {
            command: CaptureCommands::Text(ref args)
        } if args.text == ["Pixel", "OpenClaw", "Symphony"]
            && args.instructions.as_deref() == Some("Summarize the stack")
            && args.tags == ["pixel", "#OpenClaw,pixel"]
    ));
}

#[test]
fn parses_search_command_with_filters() {
    let cli = Cli::try_parse_from([
        "smesh",
        "--json",
        "search",
        "--top-k",
        "3",
        "--filter",
        "Source,Note",
        "--date-from",
        "2026-01-01",
        "--date-to",
        "2026-02-01",
        "Pixel",
        "Symphony",
        "vision",
    ])
    .expect("valid search command");

    assert!(matches!(
        cli.command,
        Commands::Search(ref args)
            if args.query == ["Pixel", "Symphony", "vision"]
                && args.top_k == 3
                && args.filters == ["Source,Note"]
                && args.date_from.as_deref() == Some("2026-01-01")
                && args.date_to.as_deref() == Some("2026-02-01")
    ));
    assert_eq!(cli.effective_output(), OutputMode::Json);
}

#[test]
fn parses_ask_command() {
    let cli =
        Cli::try_parse_from(["smesh", "ask", "What", "is", "Pixel?"]).expect("valid ask command");

    assert!(matches!(
        cli.command,
        Commands::Ask(ref args) if args.question == ["What", "is", "Pixel?"]
    ));
}

#[test]
fn parses_capture_file_command_with_mime_override() {
    let cli = Cli::try_parse_from([
        "smesh",
        "capture",
        "file",
        "/tmp/stack.capture",
        "--mesh-id",
        "mesh-test",
        "--instructions",
        "Summarize the image",
        "--tag",
        "image",
        "--mime-type",
        "image/png",
    ])
    .expect("valid capture file command");

    assert!(matches!(
        cli.command,
        Commands::Capture {
            command: CaptureCommands::File(ref args)
        } if args.path == PathBuf::from("/tmp/stack.capture")
            && args.instructions.as_deref() == Some("Summarize the image")
            && args.tags == ["image"]
            && args.mime_type.as_deref() == Some("image/png")
    ));
    assert_eq!(cli.mesh_id.as_deref(), Some("mesh-test"));
}

#[test]
fn capture_text_posts_json_and_outputs_agent_readable_shape() {
    let (api_url, requests, handle) = start_json_server(
        r#"{"task_id":"job-text","capture_id":"capture-text","details":{"synapse_id":"text_capture@1.0.0"}}"#,
    );
    let config = temp_config_path("capture-text");

    let output = smesh_command()
        .arg("--json")
        .arg("--api-url")
        .arg(&api_url)
        .arg("--config")
        .arg(&config)
        .arg("--token")
        .arg("token-test")
        .arg("--mesh-id")
        .arg("mesh-test")
        .arg("capture")
        .arg("text")
        .arg("Pixel")
        .arg("OpenClaw")
        .arg("Symphony")
        .arg("--instructions")
        .arg("Summarize the stack")
        .arg("--tag")
        .arg("pixel,#OpenClaw,pixel")
        .output()
        .expect("run capture text");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    assert_eq!(value["job_id"], "job-text");
    assert_eq!(value["operation_id"], "job-text");
    assert_eq!(value["capture_id"], "capture-text");
    assert_eq!(value["status"], "queued");
    assert_eq!(value["file_ids"], json!([]));
    assert_eq!(value["source_links"], json!([]));
    assert_eq!(value["details"]["capture_type"], "text");
    assert_eq!(value["details"]["mesh_id"], "mesh-test");
    assert_eq!(value["details"]["tags"], json!(["pixel", "OpenClaw"]));
    assert_eq!(value["links"]["job"], "/v1/jobs/job-text");
    assert_eq!(value["links"]["capture"], "/v1/captures/capture-text");

    let request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("captured request");
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/cli/capture/text");
    assert_eq!(
        request.headers.get("authorization").map(String::as_str),
        Some("Bearer token-test")
    );
    assert_eq!(
        request.headers.get("x-mesh-context").map(String::as_str),
        Some("mesh-test")
    );
    assert!(request
        .headers
        .get("content-type")
        .expect("content-type")
        .starts_with("application/json"));

    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
    assert_eq!(body["text"], "Pixel OpenClaw Symphony");
    assert_eq!(body["mesh_id"], "mesh-test");
    assert_eq!(body["instructions"], "Summarize the stack");
    assert_eq!(body["tags"], json!(["pixel", "OpenClaw"]));

    handle.join().expect("server thread");
}

#[test]
fn capture_file_posts_multipart_and_outputs_file_summary() {
    let (api_url, requests, handle) = start_json_server(
        r#"{"task_id":"job-file","capture_id":"capture-file","file_id":"file-test","details":{"source_links":["/v1/sources/source-test"]}}"#,
    );
    let config = temp_config_path("capture-file");
    let temp_dir = temp_data_path("capture-file-dir");
    fs::create_dir_all(&temp_dir).expect("create temp dir");
    let file_path = temp_dir.join("stack \"image\".capture");
    let image_bytes = b"\x89PNG\r\n\x1a\npixel-openclaw-symphony";
    fs::write(&file_path, image_bytes).expect("write image file");

    let output = smesh_command()
        .arg("--json")
        .arg("--api-url")
        .arg(&api_url)
        .arg("--config")
        .arg(&config)
        .arg("--token")
        .arg("token-test")
        .arg("--mesh-id")
        .arg("mesh-test")
        .arg("capture")
        .arg("file")
        .arg(&file_path)
        .arg("--instructions")
        .arg("Summarize the stack image")
        .arg("--tag")
        .arg("pixel,#OpenClaw,pixel")
        .output()
        .expect("run capture file");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    assert_eq!(value["job_id"], "job-file");
    assert_eq!(value["operation_id"], "job-file");
    assert_eq!(value["capture_id"], "capture-file");
    assert_eq!(value["status"], "queued");
    assert_eq!(value["file_id"], "file-test");
    assert_eq!(value["file_ids"], json!(["file-test"]));
    assert_eq!(value["source_links"], json!(["/v1/sources/source-test"]));
    assert_eq!(value["details"]["capture_type"], "file");
    assert_eq!(value["details"]["mesh_id"], "mesh-test");
    assert_eq!(value["details"]["filename"], "stack \"image\".capture");
    assert_eq!(value["details"]["content_type"], "image/png");
    assert_eq!(value["details"]["tags"], json!(["pixel", "OpenClaw"]));

    let request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("captured request");
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/cli/capture/file");
    assert_eq!(
        request.headers.get("authorization").map(String::as_str),
        Some("Bearer token-test")
    );
    assert_eq!(
        request.headers.get("x-mesh-context").map(String::as_str),
        Some("mesh-test")
    );
    assert!(request
        .headers
        .get("content-type")
        .expect("content-type")
        .starts_with("multipart/form-data; boundary="));

    let body = String::from_utf8_lossy(&request.body);
    assert!(body.contains("name=\"instructions\""));
    assert!(body.contains("Summarize the stack image"));
    assert!(body.contains("name=\"tags\""));
    assert!(body.contains(r#"["pixel","OpenClaw"]"#));
    assert!(body.contains("filename=\"stack \\\"image\\\".capture\""));
    assert!(body.contains("filename*=UTF-8''stack%20%22image%22.capture"));
    assert!(body.contains("Content-Type: image/png"));
    assert!(request
        .body
        .windows(image_bytes.len())
        .any(|window| window == image_bytes));

    handle.join().expect("server thread");
}

#[test]
fn capture_file_human_output_includes_useful_summary() {
    let (api_url, _requests, handle) = start_json_server(
        r#"{"job_id":"job-human","capture_id":"capture-human","file_id":"file-human","status":"queued"}"#,
    );
    let config = temp_config_path("capture-file-human");
    let temp_dir = temp_data_path("capture-file-human-dir");
    fs::create_dir_all(&temp_dir).expect("create temp dir");
    let file_path = temp_dir.join("stack.png");
    fs::write(&file_path, b"\x89PNG\r\n\x1a\nhuman-summary").expect("write image file");

    let output = smesh_command()
        .arg("--output")
        .arg("human")
        .arg("--api-url")
        .arg(&api_url)
        .arg("--config")
        .arg(&config)
        .arg("--token")
        .arg("token-test")
        .arg("--mesh-id")
        .arg("mesh-test")
        .arg("capture")
        .arg("file")
        .arg(&file_path)
        .output()
        .expect("run capture file");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Capture queued."));
    assert!(stdout.contains("Operation ID: job-human"));
    assert!(stdout.contains("Status: queued"));
    assert!(stdout.contains("Job ID: job-human"));
    assert!(stdout.contains("Capture ID: capture-human"));
    assert!(stdout.contains("File ID(s): file-human"));
    assert!(stdout.contains("File: stack.png (image/png)"));

    handle.join().expect("server thread");
}

#[test]
fn parses_topics_query_command() {
    let cli = Cli::try_parse_from([
        "smesh",
        "--output",
        "ndjson",
        "topics",
        "query",
        "--topic",
        "Pixel,Symphony",
        "--match",
        "all",
        "--limit",
        "5",
        "--offset",
        "2",
        "--recent",
        "--exclude-node-type",
        "Source",
        "--since",
        "2026-01-01",
    ])
    .expect("valid topics query command");

    assert!(matches!(
        cli.command,
        Commands::Topics {
            command: TopicCommands::Query(ref args)
        } if args.common.topics == ["Pixel,Symphony"]
            && args.common.match_mode == TopicMatch::All
            && args.limit == 5
            && args.offset == 2
            && args.recent
            && args.sort == TopicSort::Relevance
            && args.common.exclude_node_types == ["Source"]
            && args.common.since.as_deref() == Some("2026-01-01")
    ));
    assert_eq!(cli.effective_output(), OutputMode::Ndjson);
}

#[test]
fn search_json_uses_stable_schema_and_v1_fallback() {
    let server = MockServer::start(vec![
        MockResponse::json(404, r#"{"detail":"missing"}"#),
        MockResponse::json(
            200,
            r#"{
              "results": [
                {
                  "node_type": "Source",
                  "score": 0.97,
                  "distance": 0.03,
                  "snippet": "Captured vision snippet",
                  "data": {
                    "id": "node-1",
                    "source_id": "source-1",
                    "title": "Pixel/Symphony Vision"
                  }
                }
              ]
            }"#,
        ),
    ]);

    let output = smesh_command()
        .arg("--api-url")
        .arg(server.url())
        .arg("--token")
        .arg("test-token")
        .arg("--mesh-id")
        .arg("mesh-1")
        .arg("--json")
        .arg("search")
        .arg("--top-k")
        .arg("2")
        .arg("--filter")
        .arg("Source,Note")
        .arg("Pixel")
        .arg("Symphony")
        .arg("vision")
        .output()
        .expect("run search");

    let requests = server.join();
    assert!(output.status.success());
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/cli/search");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/v1/search");
    assert_eq!(
        requests[1].authorization.as_deref(),
        Some("Bearer test-token")
    );
    assert_eq!(requests[1].mesh_id.as_deref(), Some("mesh-1"));
    let payload: serde_json::Value =
        serde_json::from_str(&requests[1].body).expect("request JSON body");
    assert_eq!(
        payload,
        json!({
            "query": "Pixel Symphony vision",
            "top_k": 2,
            "filters": {
                "labels": ["Source", "Note"]
            }
        })
    );

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(
        value,
        json!({
            "schema_version": 1,
            "query": "Pixel Symphony vision",
            "top_k": 2,
            "filters": {
                "labels": ["Source", "Note"],
                "date_from": null,
                "date_to": null
            },
            "result_count": 1,
            "results": [
                {
                    "rank": 1,
                    "id": "node-1",
                    "node_type": "Source",
                    "title": "Pixel/Symphony Vision",
                    "snippet": "Captured vision snippet",
                    "score": 0.97,
                    "distance": 0.03,
                    "data": {
                        "id": "node-1",
                        "source_id": "source-1",
                        "title": "Pixel/Symphony Vision"
                    }
                }
            ]
        })
    );
}

#[test]
fn ask_json_uses_stable_schema() {
    let server = MockServer::start(vec![
        MockResponse::json(404, r#"{"detail":"missing"}"#),
        MockResponse::json(
            200,
            r#"{
              "answer": "Pixel, Symphony, and ScientiaMesh share a retrieval vision.",
              "source_ids": ["source-1"],
              "sources": [{"id": "source-1", "excerpt": "vision"}],
              "report_ids": ["report-1"],
              "suggested_title": "Shared Vision"
            }"#,
        ),
    ]);

    let output = smesh_command()
        .arg("--api-url")
        .arg(server.url())
        .arg("--token")
        .arg("test-token")
        .arg("--mesh-id")
        .arg("mesh-1")
        .arg("--json")
        .arg("ask")
        .arg("What")
        .arg("is")
        .arg("the")
        .arg("vision?")
        .output()
        .expect("run ask");

    let requests = server.join();
    assert!(output.status.success());
    assert_eq!(requests[0].path, "/api/cli/ask");
    assert_eq!(requests[1].path, "/v1/ask");
    let payload: serde_json::Value =
        serde_json::from_str(&requests[1].body).expect("request JSON body");
    assert_eq!(
        payload,
        json!({
            "question": "What is the vision?",
            "stream": false
        })
    );

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(
        value,
        json!({
            "schema_version": 1,
            "question": "What is the vision?",
            "answer": "Pixel, Symphony, and ScientiaMesh share a retrieval vision.",
            "source_ids": ["source-1"],
            "sources": [{"id": "source-1", "excerpt": "vision"}],
            "report_ids": ["report-1"],
            "suggested_title": "Shared Vision"
        })
    );
}

#[test]
fn topics_query_json_uses_stable_schema() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        r#"{
          "items": [
            {
              "id": "topic-node-1",
              "node_type": "Note",
              "title": "ScientiaMesh Vision",
              "snippet": "Pixel and Symphony retrieval context",
              "updatedAt": "2026-05-01T12:00:00Z",
              "createdAt": "2026-04-30T12:00:00Z",
              "matched_topics": ["Pixel", "Symphony"]
            }
          ],
          "paging": {
            "limit": 2,
            "offset": 0,
            "total": 1,
            "has_more": false
          }
        }"#,
    )]);

    let output = smesh_command()
        .arg("--api-url")
        .arg(server.url())
        .arg("--token")
        .arg("test-token")
        .arg("--mesh-id")
        .arg("mesh-1")
        .arg("--json")
        .arg("topics")
        .arg("query")
        .arg("--topic")
        .arg("Pixel,Symphony")
        .arg("--match")
        .arg("all")
        .arg("--limit")
        .arg("2")
        .arg("--sort")
        .arg("recent")
        .arg("--exclude-node-type")
        .arg("Source")
        .arg("--since")
        .arg("2026-04-01")
        .output()
        .expect("run topics query");

    let requests = server.join();
    assert!(output.status.success());
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/topics/query");
    let payload: serde_json::Value =
        serde_json::from_str(&requests[0].body).expect("request JSON body");
    assert_eq!(
        payload,
        json!({
            "topics": ["Pixel", "Symphony"],
            "match": "all",
            "limit": 2,
            "offset": 0,
            "sort": "recent",
            "exclude_node_types": ["Source"],
            "window": {
                "since": "2026-04-01"
            }
        })
    );

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(
        value,
        json!({
            "schema_version": 1,
            "topics": ["Pixel", "Symphony"],
            "match": "all",
            "limit": 2,
            "offset": 0,
            "sort": "recent",
            "window": {
                "since": "2026-04-01",
                "until": null
            },
            "exclude_node_types": ["Source"],
            "items": [
                {
                    "id": "topic-node-1",
                    "node_type": "Note",
                    "title": "ScientiaMesh Vision",
                    "snippet": "Pixel and Symphony retrieval context",
                    "updatedAt": "2026-05-01T12:00:00Z",
                    "createdAt": "2026-04-30T12:00:00Z",
                    "matched_topics": ["Pixel", "Symphony"]
                }
            ],
            "paging": {
                "limit": 2,
                "offset": 0,
                "total": 1,
                "has_more": false
            }
        })
    );
}

#[test]
fn retrieval_requires_mesh_context_before_network_call() {
    let output = smesh_command()
        .arg("--api-url")
        .arg("http://127.0.0.1:9")
        .arg("--token")
        .arg("test-token")
        .arg("--json")
        .arg("search")
        .arg("Pixel")
        .output()
        .expect("run search without mesh");

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON error");
    assert!(value["error"]
        .as_str()
        .expect("error string")
        .contains("Missing mesh context"));
    assert_eq!(value["status"], serde_json::Value::Null);
}

#[test]
fn auth_status_json_reports_env_token_without_secret() {
    let config = temp_config_path("env-status");
    let output = smesh_command()
        .arg("--json")
        .arg("--config")
        .arg(&config)
        .arg("--mesh-id")
        .arg("7dd5448e-132a-45e6-8ed5-c19ae4189d30")
        .arg("auth")
        .arg("status")
        .env("SMESH_TOKEN", "env-token-for-test")
        .output()
        .expect("run auth status");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(!stdout.contains("env-token-for-test"));
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    assert_eq!(
        value,
        json!({
            "authenticated": true,
            "access_token_present": true,
            "access_token_expired": false,
            "audience": "https://api.preview.scientiamesh.app",
            "domain": "preview-smesh.ca.auth0.com",
            "api_url": "https://portal.preview.scientiamesh.app",
            "mesh_id": "7dd5448e-132a-45e6-8ed5-c19ae4189d30",
            "sub": null,
            "email": null,
            "name": null
        })
    );
}

#[test]
fn auth_status_ndjson_serializes_event_shape() {
    let config = temp_config_path("ndjson-status");
    let output = smesh_command()
        .arg("--output")
        .arg("ndjson")
        .arg("--config")
        .arg(&config)
        .arg("auth")
        .arg("status")
        .output()
        .expect("run auth status ndjson");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid NDJSON");

    assert_eq!(value["type"], "auth.status");
    assert_eq!(value["authenticated"], false);
    assert_eq!(value["access_token_present"], false);
}

#[test]
fn auth_login_without_token_returns_json_error() {
    let config = temp_config_path("unsupported-login");
    let output = smesh_command()
        .arg("--json")
        .arg("--config")
        .arg(&config)
        .arg("auth")
        .arg("login")
        .output()
        .expect("run auth login");

    assert_eq!(output.status.code(), Some(6));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON error");

    assert!(value["error"]
        .as_str()
        .expect("error string")
        .contains("Browser login is not yet wired"));
    assert_eq!(value["status"], serde_json::Value::Null);
    assert_eq!(value["details"], serde_json::Value::Null);
}

#[test]
fn auth_login_access_token_writes_config_without_printing_secret() {
    let config = temp_config_path("token-login");
    let output = smesh_command()
        .arg("--json")
        .arg("--config")
        .arg(&config)
        .arg("--mesh-id")
        .arg("7dd5448e-132a-45e6-8ed5-c19ae4189d30")
        .arg("auth")
        .arg("login")
        .arg("--access-token")
        .arg("stored-token-for-test")
        .arg("--refresh-token")
        .arg("stored-refresh-for-test")
        .arg("--expires-at")
        .arg("4102444800")
        .output()
        .expect("run token login");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(!stdout.contains("stored-token-for-test"));
    assert!(!stdout.contains("stored-refresh-for-test"));
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    assert_eq!(value["status"], "logged_in");
    assert!(value["operation_id"]
        .as_str()
        .expect("operation id")
        .starts_with("auth-login-"));
    assert_eq!(value["access_token_present"], true);
    assert_eq!(value["refresh_token_present"], true);

    let config_text = fs::read_to_string(&config).expect("written config");
    assert!(config_text.contains("stored-token-for-test"));
    assert!(config_text.contains("stored-refresh-for-test"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = fs::metadata(&config)
            .expect("config metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn auth_logout_json_outputs_status_and_removes_stored_tokens() {
    let config = temp_config_path("logout");
    fs::write(
        &config,
        r#"{
  "version": 1,
  "active_profile": "default",
  "profiles": {
    "default": {
      "api_url": "https://portal.preview.scientiamesh.app",
      "mesh_id": "7dd5448e-132a-45e6-8ed5-c19ae4189d30",
      "auth": {
        "access_token": "stored-token-for-test",
        "refresh_token": "stored-refresh-for-test",
        "expires_at": 4102444800,
        "token_type": "Bearer"
      }
    }
  }
}"#,
    )
    .expect("seed config");

    let output = smesh_command()
        .arg("--json")
        .arg("--config")
        .arg(&config)
        .arg("auth")
        .arg("logout")
        .output()
        .expect("run logout");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    assert_eq!(value["status"], "logged_out");
    assert!(value["operation_id"]
        .as_str()
        .expect("operation id")
        .starts_with("auth-logout-"));

    let config_text = fs::read_to_string(&config).expect("updated config");
    assert!(!config_text.contains("stored-token-for-test"));
    assert!(!config_text.contains("stored-refresh-for-test"));
}

#[test]
fn auth_status_config_expired_token_is_not_authenticated() {
    let config = temp_config_path("expired-status");
    fs::write(
        &config,
        r#"{
  "version": 1,
  "active_profile": "default",
  "profiles": {
    "default": {
      "api_url": "https://portal.preview.scientiamesh.app",
      "mesh_id": "7dd5448e-132a-45e6-8ed5-c19ae4189d30",
      "auth": {
        "access_token": "expired-token-for-test",
        "expires_at": 1,
        "token_type": "Bearer"
      },
      "auth_settings": {
        "domain": "preview-smesh.ca.auth0.com",
        "audience": "https://api.preview.scientiamesh.app"
      }
    }
  }
}"#,
    )
    .expect("seed config");

    let output = smesh_command()
        .arg("--json")
        .arg("--config")
        .arg(&config)
        .arg("auth")
        .arg("status")
        .output()
        .expect("run auth status");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    assert_eq!(value["authenticated"], false);
    assert_eq!(value["access_token_present"], true);
    assert_eq!(value["access_token_expired"], true);
}

fn smesh_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_smesh"));
    for name in [
        "SMESH_API_KEY",
        "SMESH_API_URL",
        "SMESH_AUTH0_AUDIENCE",
        "SMESH_AUTH0_CLIENT_ID",
        "SMESH_AUTH0_DOMAIN",
        "SMESH_CONFIG",
        "SMESH_MESH_ID",
        "SMESH_AGENT_MODE",
        "SMESH_TOKEN",
        "AUTH0_ACCESS_TOKEN",
        "AUTH0_AUDIENCE",
        "AUTH0_CLI_CLIENT_ID",
        "AUTH0_CLIENT_ID",
        "AUTH0_DOMAIN",
        "AUTH0_MCP_CLIENT_ID",
        "NEXT_PUBLIC_AUTH0_AUDIENCE",
        "NEXT_PUBLIC_AUTH0_CLIENT_ID",
        "NEXT_PUBLIC_AUTH0_DOMAIN",
    ] {
        command.env_remove(name);
    }
    command
}

fn agent_manifest_with_artifacts(artifacts: Vec<serde_json::Value>) -> String {
    json!({
        "schema_version": 1,
        "agent_name": "Pixel",
        "agent_id": "Pixel",
        "generated_at_unix_seconds": 1770000000,
        "mesh_id": "mesh-test",
        "workspace": {
            "path": "/tmp/source-workspace",
            "host": "host-test"
        },
        "index": {
            "path": ".agent-pixel.md",
            "kind": "index",
            "format": "md",
            "content": "# Pixel Portable Agent Index\n",
            "sha256": "index-sha",
            "generated_at_unix_seconds": 1770000000
        },
        "artifacts": artifacts,
    })
    .to_string()
}

fn agent_manifest_artifact(path: &str, kind: &str, content: &str) -> serde_json::Value {
    json!({
        "path": path,
        "kind": kind,
        "format": "md",
        "content": content,
        "sha256": "artifact-sha",
        "size_bytes": content.len(),
        "captured_at_unix_seconds": 1770000000,
        "source": {
            "workspace_path": "/tmp/source-workspace",
            "host": "host-test"
        }
    })
}

#[derive(Clone, Debug)]
struct MockResponse {
    status: u16,
    body: String,
}

impl MockResponse {
    fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
        }
    }
}

#[derive(Clone, Debug)]
struct MockRequest {
    method: String,
    path: String,
    authorization: Option<String>,
    mesh_id: Option<String>,
    body: String,
}

struct MockServer {
    url: String,
    requests: Arc<Mutex<Vec<MockRequest>>>,
    handle: JoinHandle<()>,
}

impl MockServer {
    fn start(responses: Vec<MockResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        listener
            .set_nonblocking(true)
            .expect("set nonblocking mock listener");
        let url = format!("http://{}", listener.local_addr().expect("local addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let thread_requests = Arc::clone(&requests);
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            for response in responses {
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(accepted) => break accepted,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "timed out waiting for request");
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("mock server accept failed: {error}"),
                    }
                };
                let request = read_mock_request(&mut stream);
                thread_requests
                    .lock()
                    .expect("lock mock requests")
                    .push(request);
                write_mock_response(&mut stream, &response);
            }
        });

        Self {
            url,
            requests,
            handle,
        }
    }

    fn url(&self) -> &str {
        &self.url
    }

    fn join(self) -> Vec<MockRequest> {
        self.handle.join().expect("mock server thread");
        self.requests.lock().expect("lock mock requests").clone()
    }
}

fn read_mock_request(stream: &mut TcpStream) -> MockRequest {
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .expect("read request line");
    let mut method = String::new();
    let mut path = String::new();
    let mut parts = request_line.split_whitespace();
    if let Some(value) = parts.next() {
        method = value.to_string();
    }
    if let Some(value) = parts.next() {
        path = value.to_string();
    }

    let mut content_length = 0usize;
    let mut authorization = None;
    let mut mesh_id = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read header line");
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            match name.as_str() {
                "content-length" => {
                    content_length = value.parse().expect("content length");
                }
                "authorization" => authorization = Some(value),
                "x-mesh-context" => mesh_id = Some(value),
                _ => {}
            }
        }
    }

    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).expect("read body");
    MockRequest {
        method,
        path,
        authorization,
        mesh_id,
        body: String::from_utf8(body).expect("utf8 request body"),
    }
}

fn write_mock_response(stream: &mut TcpStream, response: &MockResponse) {
    let reason = match response.status {
        200 => "OK",
        404 => "Not Found",
        _ => "OK",
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        reason,
        response.body.len(),
        response.body
    )
    .expect("write mock response");
}

fn temp_config_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "smesh-rs-{name}-{}-{nanos}.json",
        std::process::id()
    ))
}

fn temp_data_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("smesh-rs-{}-{nanos}-{name}", std::process::id()))
}

#[derive(Debug)]
struct CapturedRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn start_json_server(
    response_body: &'static str,
) -> (String, Receiver<CapturedRequest>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let address = listener.local_addr().expect("local address");
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let request = read_http_request(&mut stream);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
        sender.send(request).expect("send captured request");
    });

    (format!("http://{address}"), receiver, handle)
}

fn read_http_request(stream: &mut TcpStream) -> CapturedRequest {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk).expect("read request");
        assert_ne!(read, 0, "request ended before headers");
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_header_end(&buffer) {
            break index;
        }
    };

    let header_text = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().expect("request line");
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().expect("method").to_string();
    let path = request_parts.next().expect("path").to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();
    let body_start = header_end + 4;
    let total_length = body_start + content_length;
    while buffer.len() < total_length {
        let read = stream.read(&mut chunk).expect("read body");
        assert_ne!(read, 0, "request ended before body");
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = buffer[body_start..total_length].to_vec();

    CapturedRequest {
        method,
        path,
        headers,
        body,
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}
