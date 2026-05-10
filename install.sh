#!/usr/bin/env sh
set -eu

DOWNLOAD_BASE="${SMESH_DOWNLOAD_BASE:-https://download.scientiamesh.app}"
DOWNLOAD_BASE="${DOWNLOAD_BASE%/}"
INSTALL_DIR="${SMESH_INSTALL_DIR:-$HOME/.local/bin}"
INSTALL_NAME="${SMESH_INSTALL_NAME:-smesh}"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "smesh installer: missing required command: $1" >&2
    exit 1
  fi
}

detect_platform() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin) os_slug="macos" ;;
    Linux) os_slug="linux" ;;
    *)
      echo "smesh installer: unsupported OS: $os" >&2
      exit 1
      ;;
  esac

  case "$arch" in
    arm64 | aarch64) arch_slug="arm64" ;;
    x86_64 | amd64) arch_slug="x64" ;;
    *)
      echo "smesh installer: unsupported architecture: $arch" >&2
      exit 1
      ;;
  esac

  if [ "$os_slug" = "linux" ] && [ "$arch_slug" = "arm64" ]; then
    echo "smesh installer: linux-arm64 binary is not published yet" >&2
    exit 1
  fi

  printf '%s-%s' "$os_slug" "$arch_slug"
}

asset_url() {
  platform="$1"
  case "$DOWNLOAD_BASE" in
    *github.com*/releases/download/*)
      printf '%s/smesh-%s' "$DOWNLOAD_BASE" "$platform"
      ;;
    *)
      printf '%s/smesh/latest/%s' "$DOWNLOAD_BASE" "$platform"
      ;;
  esac
}

checksum_cmd() {
  if command -v sha256sum >/dev/null 2>&1; then
    printf 'sha256sum'
    return
  fi
  if command -v shasum >/dev/null 2>&1; then
    printf 'shasum -a 256'
    return
  fi
  echo "smesh installer: missing sha256sum or shasum" >&2
  exit 1
}

need curl
need chmod
need mkdir
need mktemp

platform="$(detect_platform)"
url="$(asset_url "$platform")"
tmp_dir="$(mktemp -d)"
bin_path="$tmp_dir/$INSTALL_NAME"
checksum_path="$tmp_dir/$INSTALL_NAME.sha256"
checksum_runner="$(checksum_cmd)"

cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

curl -fsSL "$url" -o "$bin_path"
curl -fsSL "$url.sha256" -o "$checksum_path"

expected="$(awk '{print $1}' "$checksum_path")"
actual="$($checksum_runner "$bin_path" | awk '{print $1}')"

if [ "$expected" != "$actual" ]; then
  echo "smesh installer: checksum mismatch for $url" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
chmod +x "$bin_path"
mv "$bin_path" "$INSTALL_DIR/$INSTALL_NAME"

echo "Installed smesh to $INSTALL_DIR/$INSTALL_NAME"
