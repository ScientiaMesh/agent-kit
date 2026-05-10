#!/usr/bin/env python3
"""Build smesh CLI and ScientiaMesh EA release manifests."""

from __future__ import annotations

import argparse
import calendar
import gzip
import hashlib
import json
import os
import shutil
import subprocess
import tarfile
import time
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
CLI_MANIFEST_NAME = "smesh-latest.json"
SKILL_MANIFEST_NAME = "scientiamesh-ea-latest.json"


@dataclass(frozen=True)
class Platform:
    slug: str
    os: str
    arch: str
    rust_target: str
    filename: str


PLATFORMS = (
    Platform(
        slug="macos-arm64",
        os="macos",
        arch="arm64",
        rust_target="aarch64-apple-darwin",
        filename="smesh-macos-arm64",
    ),
    Platform(
        slug="macos-x64",
        os="macos",
        arch="x64",
        rust_target="x86_64-apple-darwin",
        filename="smesh-macos-x64",
    ),
    Platform(
        slug="linux-x64",
        os="linux",
        arch="x64",
        rust_target="x86_64-unknown-linux-gnu",
        filename="smesh-linux-x64",
    ),
    Platform(
        slug="windows-x64",
        os="windows",
        arch="x64",
        rust_target="x86_64-pc-windows-msvc",
        filename="smesh-windows-x64.exe",
    ),
)


def utc_now() -> str:
    epoch = (os.getenv("SOURCE_DATE_EPOCH") or "").strip()
    if epoch.isdigit():
        return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(int(epoch)))
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def parse_generated_epoch(generated_at: str) -> int:
    try:
        parsed = time.strptime(
            generated_at.replace("Z", ""),
            "%Y-%m-%dT%H:%M:%S",
        )
        return calendar.timegm(parsed)
    except Exception:
        return 0


def git_sha() -> str:
    for key in ("SMESH_GIT_SHA", "GITHUB_SHA", "VERCEL_GIT_COMMIT_SHA"):
        value = (os.getenv(key) or "").strip()
        if value:
            return value
    try:
        result = subprocess.run(
            ["git", "-C", str(ROOT), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
            timeout=2,
        )
    except Exception:
        return "unknown"
    return result.stdout.strip() or "unknown"


def cli_version() -> str:
    with (ROOT / "apps" / "cli-rust" / "Cargo.toml").open("rb") as handle:
        cargo = tomllib.load(handle)
    return cargo["package"]["version"]


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def write_checksum(path: Path, digest: str, filename: str) -> None:
    path.write_text(f"{digest}  {filename}\n", encoding="utf-8")


def public_cli_url(base_url: str, channel: str, platform: Platform) -> str:
    if "github.com" in base_url and "/releases/download/" in base_url:
        return f"{base_url}/{platform.filename}"
    return f"{base_url}/smesh/{channel}/{platform.slug}"


def public_skill_url(base_url: str, channel: str, suffix: str) -> str:
    if "github.com" in base_url and "/releases/download/" in base_url:
        if suffix == ".tar.gz":
            return f"{base_url}/scientiamesh-ea-{channel}.tar.gz"
        if suffix == "":
            return f"{base_url}/scientiamesh-ea-{channel}"
        return f"{base_url}/scientiamesh-ea-{channel}{suffix}"
    return f"{base_url}/skills/scientiamesh-ea/{channel}{suffix}"


def copy_installer(output_dir: Path) -> tuple[Path, str]:
    installer = output_dir / "install.sh"
    shutil.copyfile(ROOT / "scripts" / "install_smesh.sh", installer)
    installer.chmod(0o755)
    digest = sha256(installer)
    write_checksum(output_dir / "install.sh.sha256", digest, "install.sh")
    return installer, digest


def ensure_cli_assets(
    assets_dir: Path,
    output_dir: Path,
    fail_missing: bool,
) -> dict[str, dict[str, Any]]:
    platforms: dict[str, dict[str, Any]] = {}

    for platform in PLATFORMS:
        source = assets_dir / platform.filename
        target = output_dir / platform.filename
        if not source.exists():
            if fail_missing:
                raise FileNotFoundError(f"missing CLI asset: {source}")
            continue
        if source.resolve() != target.resolve():
            shutil.copyfile(source, target)
        if platform.os != "windows":
            target.chmod(0o755)

        digest = sha256(target)
        write_checksum(
            output_dir / f"{platform.filename}.sha256",
            digest,
            platform.filename,
        )
        platforms[platform.slug] = {
            "arch": platform.arch,
            "filename": platform.filename,
            "os": platform.os,
            "rust_target": platform.rust_target,
            "sha256": digest,
        }

    checksum_lines = [
        f"{data['sha256']}  {data['filename']}"
        for _, data in sorted(platforms.items())
    ]
    (output_dir / "smesh-checksums.txt").write_text(
        "\n".join(checksum_lines) + "\n",
        encoding="utf-8",
    )
    return platforms


def build_cli_manifest(
    *,
    base_url: str,
    channel: str,
    version: str,
    commit_sha: str,
    generated_at: str,
    platforms: dict[str, dict[str, Any]],
    installer_sha256: str,
) -> dict[str, Any]:
    version_channel = f"v{version}"
    platform_payload: dict[str, dict[str, Any]] = {}

    by_slug = {platform.slug: platform for platform in PLATFORMS}
    for slug, data in sorted(platforms.items()):
        platform = by_slug[slug]
        url = public_cli_url(base_url, channel, platform)
        platform_payload[slug] = {
            **data,
            "sha256_url": f"{url}.sha256",
            "url": url,
            "versioned_url": public_cli_url(
                base_url, version_channel, platform
            ),
        }

    return {
        "schema_version": 1,
        "kind": "smesh-cli",
        "name": "smesh",
        "version": version,
        "channel": channel,
        "commit_sha": commit_sha,
        "generated_at": generated_at,
        "backing_store": "github_releases",
        "download_base_url": base_url,
        "install": {
            "script_url": f"{base_url}/install.sh",
            "sha256": installer_sha256,
            "sha256_url": f"{base_url}/install.sh.sha256",
        },
        "checksums": {
            "sha256_url": f"{base_url}/smesh/{channel}/checksums.txt",
            "filename": "smesh-checksums.txt",
        },
        "platforms": platform_payload,
    }


def load_skill_metadata(skill_source: Path) -> dict[str, Any]:
    metadata_path = skill_source / "skill.json"
    with metadata_path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def tar_add_file(
    tar: tarfile.TarFile,
    source: Path,
    arcname: str,
    mtime: int,
) -> None:
    info = tar.gettarinfo(str(source), arcname)
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    info.mtime = mtime
    info.mode = 0o644
    with source.open("rb") as handle:
        tar.addfile(info, handle)


def make_skill_bundle(
    *,
    output_dir: Path,
    skill_source: Path,
    skill_version: str,
    generated_at: str,
) -> tuple[dict[str, Any], str, str]:
    metadata = load_skill_metadata(skill_source)
    required = ["SKILL.md", "skill.json"]
    for relative in required:
        if not (skill_source / relative).is_file():
            raise FileNotFoundError(f"missing skill file: {relative}")

    versioned_name = f"scientiamesh-ea-v{skill_version}.tar.gz"
    versioned_alias_name = f"scientiamesh-ea-v{skill_version}"
    latest_name = "scientiamesh-ea-latest.tar.gz"
    latest_alias_name = "scientiamesh-ea-latest"
    versioned_path = output_dir / versioned_name
    versioned_alias_path = output_dir / versioned_alias_name
    latest_path = output_dir / latest_name
    latest_alias_path = output_dir / latest_alias_name
    mtime = parse_generated_epoch(generated_at)

    files = [
        path
        for path in sorted(skill_source.rglob("*"))
        if path.is_file() and "__pycache__" not in path.parts
    ]

    with versioned_path.open("wb") as raw:
        with gzip.GzipFile(
            filename="",
            mode="wb",
            fileobj=raw,
            mtime=mtime,
        ) as gz:
            with tarfile.open(fileobj=gz, mode="w") as tar:
                for path in files:
                    rel = path.relative_to(skill_source).as_posix()
                    tar_add_file(tar, path, f"scientiamesh-ea/{rel}", mtime)

    shutil.copyfile(versioned_path, latest_path)
    shutil.copyfile(versioned_path, versioned_alias_path)
    shutil.copyfile(versioned_path, latest_alias_path)
    digest = sha256(latest_path)
    write_checksum(
        output_dir / f"{latest_name}.sha256",
        digest,
        latest_name,
    )
    write_checksum(
        output_dir / f"{latest_alias_name}.sha256",
        digest,
        latest_alias_name,
    )
    write_checksum(
        output_dir / f"{versioned_name}.sha256",
        digest,
        versioned_name,
    )
    write_checksum(
        output_dir / f"{versioned_alias_name}.sha256",
        digest,
        versioned_alias_name,
    )
    return metadata, latest_name, digest


def build_skill_manifest(
    *,
    base_url: str,
    channel: str,
    skill_version: str,
    commit_sha: str,
    generated_at: str,
    metadata: dict[str, Any],
    bundle_name: str,
    bundle_sha256: str,
) -> dict[str, Any]:
    version_channel = f"v{skill_version}"
    bundle_url = public_skill_url(base_url, channel, ".tar.gz")

    return {
        "schema_version": 1,
        "kind": "agent-skill",
        "name": metadata["name"],
        "title": metadata["title"],
        "version": skill_version,
        "channel": channel,
        "commit_sha": commit_sha,
        "generated_at": generated_at,
        "entrypoint": metadata["entrypoint"],
        "compatible_smesh": metadata["compatible_smesh"],
        "compatible_api": metadata["compatible_api"],
        "bundle": {
            "alias_url": public_skill_url(base_url, channel, ""),
            "filename": bundle_name,
            "sha256": bundle_sha256,
            "sha256_url": f"{bundle_url}.sha256",
            "url": bundle_url,
            "versioned_url": public_skill_url(
                base_url,
                version_channel,
                ".tar.gz",
            ),
        },
        "contents": metadata["files"],
    }


def write_global_checksums(output_dir: Path) -> None:
    checksum_files = {"SHA256SUMS"}
    paths = [
        path
        for path in sorted(output_dir.iterdir())
        if path.is_file() and path.name not in checksum_files
    ]
    lines = [f"{sha256(path)}  {path.name}" for path in paths]
    (output_dir / "SHA256SUMS").write_text(
        "\n".join(lines) + "\n",
        encoding="utf-8",
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--assets-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--version", default=cli_version())
    parser.add_argument("--skill-version")
    parser.add_argument("--commit-sha", default=git_sha())
    parser.add_argument("--generated-at", default=utc_now())
    parser.add_argument(
        "--download-base-url",
        default="https://download.scientiamesh.app",
    )
    parser.add_argument(
        "--skill-source",
        type=Path,
        default=ROOT / "agent-skills" / "scientiamesh-ea",
    )
    parser.add_argument(
        "--allow-missing-binaries",
        action="store_true",
        help="Generate skill assets even when CLI binaries are absent.",
    )
    args = parser.parse_args()

    output_dir = args.output_dir
    output_dir.mkdir(parents=True, exist_ok=True)
    args.download_base_url = args.download_base_url.rstrip("/")

    skill_metadata = load_skill_metadata(args.skill_source)
    skill_version = args.skill_version or skill_metadata["version"]

    platforms = ensure_cli_assets(
        args.assets_dir,
        output_dir,
        fail_missing=not args.allow_missing_binaries,
    )
    _, installer_sha256 = copy_installer(output_dir)

    cli_latest = build_cli_manifest(
        base_url=args.download_base_url,
        channel="latest",
        version=args.version,
        commit_sha=args.commit_sha,
        generated_at=args.generated_at,
        platforms=platforms,
        installer_sha256=installer_sha256,
    )
    write_json(output_dir / CLI_MANIFEST_NAME, cli_latest)
    write_json(
        output_dir / f"smesh-v{args.version}.json",
        {
            **cli_latest,
            "channel": f"v{args.version}",
            "checksums": {
                **cli_latest["checksums"],
                "sha256_url": (
                    f"{args.download_base_url}/smesh/v{args.version}"
                    "/checksums.txt"
                ),
            },
            "platforms": {
                slug: {
                    **data,
                    "sha256_url": f"{data['versioned_url']}.sha256",
                    "url": data["versioned_url"],
                }
                for slug, data in cli_latest["platforms"].items()
            },
        },
    )

    skill_metadata, skill_bundle_name, skill_bundle_sha256 = make_skill_bundle(
        output_dir=output_dir,
        skill_source=args.skill_source,
        skill_version=skill_version,
        generated_at=args.generated_at,
    )
    skill_latest = build_skill_manifest(
        base_url=args.download_base_url,
        channel="latest",
        skill_version=skill_version,
        commit_sha=args.commit_sha,
        generated_at=args.generated_at,
        metadata=skill_metadata,
        bundle_name=skill_bundle_name,
        bundle_sha256=skill_bundle_sha256,
    )
    write_json(output_dir / SKILL_MANIFEST_NAME, skill_latest)
    write_json(
        output_dir / f"scientiamesh-ea-v{skill_version}.json",
        {
            **skill_latest,
            "channel": f"v{skill_version}",
            "bundle": {
                **skill_latest["bundle"],
                "alias_url": (
                    f"{args.download_base_url}/skills/scientiamesh-ea/"
                    f"v{skill_version}"
                ),
                "filename": f"scientiamesh-ea-v{skill_version}.tar.gz",
                "sha256_url": (
                    f"{args.download_base_url}/skills/scientiamesh-ea/"
                    f"v{skill_version}.tar.gz.sha256"
                ),
                "url": skill_latest["bundle"]["versioned_url"],
            },
        },
    )

    write_global_checksums(output_dir)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
