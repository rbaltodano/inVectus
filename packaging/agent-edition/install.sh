#!/bin/sh
# Install the inVectus agent edition (the `invectus` command and its MCP server, no local
# language model) for the current user. No sudo, no network unless you ask for the model.
#
#   ./install.sh [--prefix DIR] [--model] [--connect CLIENT] [--yes]
#
#   --prefix DIR      install into DIR/bin (default: ~/.local)
#   --model           download the default embedding model (about 134 MB from Hugging Face,
#                     checksum-verified); asks first unless --yes
#   --connect CLIENT  claude-code, claude-desktop, codex, cursor or grok; shows the change
#                     and asks first unless --yes
set -eu

here=$(cd "$(dirname "$0")" && pwd)
prefix="$HOME/.local"
model=0
connect=""
yes=0
while [ $# -gt 0 ]; do
  case "$1" in
    --prefix) prefix="$2"; shift 2 ;;
    --model) model=1; shift ;;
    --connect) connect="$2"; shift 2 ;;
    --yes) yes=1; shift ;;
    -h|--help) sed -n '2,14p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

ask() {
  [ "$yes" -eq 1 ] && return 0
  printf '%s [y/N] ' "$1"
  read -r answer || return 1
  case "$answer" in y|Y|yes|YES) return 0 ;; *) return 1 ;; esac
}

bin="$prefix/bin"
mkdir -p "$bin"
cp "$here/invectus" "$bin/invectus.new"
chmod 755 "$bin/invectus.new"
mv "$bin/invectus.new" "$bin/invectus"
echo "installed $bin/invectus ($("$bin/invectus" --version))"
case ":$PATH:" in
  *":$bin:"*) ;;
  *) echo "note: $bin is not on your PATH; add it to use \`invectus\` by name" ;;
esac

if [ "$model" -eq 1 ]; then
  if ask "Download the embedding model bge-small-en-v1.5 (about 134 MB) from Hugging Face?"; then
    "$bin/invectus" models download bge-small-en-v1.5
  else
    echo "skipped the model; download it later with: invectus models download bge-small-en-v1.5"
  fi
fi

if [ -n "$connect" ]; then
  "$bin/invectus" connect add "$connect"
  if ask "Make this change?"; then
    "$bin/invectus" connect add "$connect" --yes
  else
    echo "not connected; run later: invectus connect add $connect --yes"
  fi
fi
