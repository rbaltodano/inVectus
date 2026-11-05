#!/bin/sh
# Build the inVectus agent edition (no local language model) and package it as
# target/agent-edition/package/invectus-agent-<version>-<os>-<arch>.tar.gz with install.sh, README, LICENSE and a
# Claude Desktop extension (.mcpb). Needs PROTOC set as for any build.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
target_dir="target/agent-edition"
cargo build --release -p invectus-cli --no-default-features --target-dir "$target_dir"
bin="$target_dir/release/invectus"
version=$("$bin" --version | awk '{print $2}')
os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
name="invectus-agent-$version-$os-$arch"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$name"
cp "$bin" "$stage/$name/invectus"
cp packaging/agent-edition/install.sh packaging/agent-edition/README.md LICENSE "$stage/$name/"
"$bin" connect bundle --out "$stage/$name/invectus.mcpb" >/dev/null
out="$target_dir/package"
mkdir -p "$out"
tar -C "$stage" -czf "$out/$name.tar.gz" "$name"
echo "wrote $out/$name.tar.gz ($(du -h "$out/$name.tar.gz" | cut -f1))"
