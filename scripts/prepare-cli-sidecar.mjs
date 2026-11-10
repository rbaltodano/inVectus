// Copy the release `invectus` CLI to src-tauri/binaries/invectus-<target triple>[.exe], where
// Tauri's `externalBin` expects it. The bundler then installs it beside the app executable as
// `invectus` (Windows install folder, Linux /usr/bin), which is where "Connect an AI agent"
// and `connect::server_binary` look for it. Used by `tauri.cli-sidecar.json` on Windows/Linux;
// the macOS bundle places the CLI with `bundle.macOS.files` instead.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const triple = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (\S+)$/m)?.[1];
if (!triple) throw new Error("could not read the host target triple from `rustc -vV`");
const exe = process.platform === "win32" ? ".exe" : "";
const from = join(root, "target", "release", `invectus${exe}`);
if (!existsSync(from)) throw new Error(`${from} is missing; run \`cargo build --release -p invectus-cli\` first`);
const to = join(root, "src-tauri", "binaries", `invectus-${triple}${exe}`);
mkdirSync(dirname(to), { recursive: true });
copyFileSync(from, to);
console.log(`CLI sidecar: ${to}`);
