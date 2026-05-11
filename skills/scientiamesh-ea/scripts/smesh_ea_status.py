#!/usr/bin/env python3
"""Inspect local ScientiaMesh EA-tool readiness without performing writes.

This script is intentionally read-only. It checks whether `smesh` exists and
whether the SCI-92 executive-assistant CLI surfaces appear to be available.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from typing import Any

EA_SUBCOMMANDS = ["tasks", "reminders", "contacts", "preferences", "briefs", "calendar"]
EXPECTED_MCP_TOOLS = {
    "tasks": [
        "smesh_tasks_create",
        "smesh_tasks_list",
        "smesh_tasks_get",
        "smesh_tasks_update",
        "smesh_tasks_complete",
        "smesh_tasks_delegate",
        "smesh_tasks_attach_source",
    ],
    "reminders": [
        "smesh_reminders_create",
        "smesh_reminders_list",
        "smesh_reminders_due_soon",
        "smesh_reminders_snooze",
        "smesh_reminders_complete",
        "smesh_reminders_dismiss",
    ],
    "contacts": [
        "smesh_contacts_people_create",
        "smesh_contacts_people_list",
        "smesh_contacts_people_get",
        "smesh_contacts_people_update",
        "smesh_contacts_orgs_create",
        "smesh_contacts_orgs_list",
        "smesh_contacts_orgs_get",
        "smesh_contacts_note_add",
        "smesh_contacts_link_source",
        "smesh_contacts_open_loops_list",
    ],
    "preferences": [
        "smesh_preferences_set",
        "smesh_preferences_list",
        "smesh_preferences_get",
        "smesh_preferences_confirm",
        "smesh_preferences_revoke",
        "smesh_preferences_evidence",
    ],
    "briefs": [
        "smesh_briefs_daily",
        "smesh_briefs_project",
        "smesh_briefs_contact",
        "smesh_briefs_meeting_prep",
        "smesh_briefs_what_changed",
    ],
    "calendar": [
        "smesh_calendar_events_list",
        "smesh_calendar_events_get",
        "smesh_calendar_events_upsert",
    ],
}


def run_cmd(argv: list[str], timeout: int = 8) -> dict[str, Any]:
    try:
        proc = subprocess.run(argv, text=True, capture_output=True, timeout=timeout)
    except FileNotFoundError:
        return {"ok": False, "returncode": None, "stdout": "", "stderr": "not found"}
    except subprocess.TimeoutExpired as exc:
        return {
            "ok": False,
            "returncode": None,
            "stdout": exc.stdout or "",
            "stderr": f"timeout after {timeout}s",
        }
    return {
        "ok": proc.returncode == 0,
        "returncode": proc.returncode,
        "stdout": proc.stdout,
        "stderr": proc.stderr,
    }


def compact(text: str, limit: int = 500) -> str:
    text = "\n".join(line.rstrip() for line in text.splitlines() if line.strip())
    return text[:limit]


def has_subcommand(root_help: str, name: str) -> bool:
    # Accept common clap/help formats without being too clever.
    needles = [f" {name} ", f" {name}\n", f"\n  {name}", f"\n    {name}"]
    padded = f"\n{root_help}\n"
    return any(n in padded for n in needles)


def build_suggestions(mesh_id: str | None) -> dict[str, list[str]]:
    mesh = mesh_id or "<mesh-id>"
    return {
        "session_start": [
            f"smesh --json tasks list --mesh-id {mesh} --status backlog,in_progress,waiting --limit 20",
            f"smesh --json reminders due-soon --mesh-id {mesh} --window PT24H",
            f"smesh --json briefs daily --mesh-id {mesh} --date YYYY-MM-DD",
        ],
        "meeting_prep": [
            f"smesh --json calendar events list --mesh-id {mesh} --from <iso-ts> --to <iso-ts>",
            f"smesh --json briefs meeting-prep <event-id> --mesh-id {mesh}",
        ],
        "fallback_retrieval": [
            f"smesh context get --agent Pixel --meshid {mesh}",
            "smesh topics query --topic scientiamesh --topic pixel --recent --format compact --summary",
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Read-only ScientiaMesh EA tool readiness check")
    parser.add_argument("--mesh-id", help="Mesh id to include in suggested commands")
    parser.add_argument("--agent", default="Pixel", help="Agent name for fallback context suggestions")
    parser.add_argument("--markdown", action="store_true", help="Print a concise markdown report instead of JSON")
    args = parser.parse_args()

    smesh_path = shutil.which("smesh")
    result: dict[str, Any] = {
        "checked_at": datetime.now(timezone.utc).isoformat(),
        "read_only": True,
        "mesh_id_supplied": bool(args.mesh_id),
        "agent": args.agent,
        "smesh": {"found": bool(smesh_path), "path": smesh_path},
        "cli": {"subcommands": {}},
        "expected_mcp_tools": EXPECTED_MCP_TOOLS,
        "suggested_commands": build_suggestions(args.mesh_id),
        "references": [
            "references/scientiamesh-ea-tool-contract.md",
            "references/fallbacks-before-ea-tools.md",
        ],
    }

    if not smesh_path:
        result["ea_cli_available"] = False
        result["recommendation"] = "Install or expose `smesh`, then use fallback local/workspace memory until ScientiaMesh retrieval is available."
    else:
        version = run_cmd([smesh_path, "--version"])
        help_out = run_cmd([smesh_path, "--help"])
        root_help = (help_out.get("stdout") or "") + "\n" + (help_out.get("stderr") or "")
        result["smesh"].update(
            {
                "version_ok": version["ok"],
                "version": compact((version.get("stdout") or version.get("stderr") or "").strip(), 200),
                "help_ok": help_out["ok"],
            }
        )

        available_count = 0
        for sub in EA_SUBCOMMANDS:
            hinted = has_subcommand(root_help, sub)
            sub_help = run_cmd([smesh_path, sub, "--help"])
            output = (sub_help.get("stdout") or "") + "\n" + (sub_help.get("stderr") or "")
            available = bool(sub_help["ok"] or hinted)
            if available:
                available_count += 1
            result["cli"]["subcommands"][sub] = {
                "available": available,
                "help_returncode": sub_help["returncode"],
                "help_excerpt": compact(output, 500),
            }

        # Optional future import helper.
        assistant_help = run_cmd([smesh_path, "assistant", "--help"])
        result["cli"]["assistant_import_markdown_hint"] = {
            "available": bool(assistant_help["ok"] and "import-markdown" in ((assistant_help.get("stdout") or "") + (assistant_help.get("stderr") or ""))),
            "help_returncode": assistant_help["returncode"],
        }
        result["ea_cli_available"] = available_count == len(EA_SUBCOMMANDS)
        if result["ea_cli_available"]:
            result["recommendation"] = "Use first-class `smesh --json` EA commands as the operational source of truth."
        elif available_count:
            result["recommendation"] = "Use available first-class commands, and use fallback ScientiaMesh capture/retrieval for missing surfaces."
        else:
            result["recommendation"] = "SCI-92 EA CLI commands are not present yet; use the fallback ScientiaMesh memory workflow with migration breadcrumbs."

    if args.markdown:
        print(f"# ScientiaMesh EA status ({result['checked_at']})")
        print(f"- smesh found: {result['smesh']['found']} ({result['smesh'].get('path')})")
        print(f"- EA CLI available: {result.get('ea_cli_available')}")
        print(f"- recommendation: {result.get('recommendation')}")
        print("\n## Subcommands")
        for sub, meta in result.get("cli", {}).get("subcommands", {}).items():
            print(f"- {sub}: {meta.get('available')}")
    else:
        print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
