# inVectus agent edition

The `invectus` command and its MCP server (`invectus mcp`) for AI agents, without the desktop
app or a local language model. Agents write gap descriptions, calibration paraphrases and
region names themselves; the engine checks them. Everything runs on this computer; the only
network use is model downloads you start and verification you switch on per project.

```sh
./install.sh                          # copies invectus to ~/.local/bin
./install.sh --model                  # also downloads the embedding model (asks first)
./install.sh --connect claude-code    # also registers with Claude Code (shows the change, asks)
```

Claude Desktop can instead open `invectus.mcpb` from this folder.

Uninstall: `invectus connect remove <client> --yes`, then delete `~/.local/bin/invectus`.
Models live in the OS app-data folder (`invectus models list` shows where).
