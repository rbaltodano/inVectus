#!/usr/bin/env node
// Offline, conservative source-license inventory. Includes build/test/platform dependencies;
// it is not a legal clearance or an inventory of prebuilt native runtime distributions.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const output = path.resolve(process.argv[2] ?? path.join(root, "target/release-notices"));
const metadata = JSON.parse(execFileSync("cargo", [
  "metadata", "--offline", "--locked", "--format-version", "1",
], { cwd: root, maxBuffer: 64 * 1024 * 1024, encoding: "utf8" }));
const packages = [];
const sections = [readFileSync(path.join(root, "THIRD_PARTY_NOTICES.md"), "utf8"),
  "Conservative source-license inventory: includes build/test/other-platform dependencies.\n" +
  "Prebuilt native runtimes and platform installer requirements need separate review.\n"];

function collect(directory) {
  const found = [];
  function walk(folder, licenseDirectory = false) {
    for (const entry of readdirSync(folder, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const file = path.join(folder, entry.name);
      if (entry.isDirectory() && ![".git", "node_modules", "target"].includes(entry.name)) {
        walk(file, licenseDirectory || /^(licen[cs]es|notices)$/i.test(entry.name));
      } else if (entry.isFile() && (licenseDirectory || /^(unlicense|licen[cs]e|notice|copying|copyright)([._-].*)?$/i.test(entry.name))) {
        found.push(file);
      }
    }
  }
  walk(directory);
  return found;
}

function add(ecosystem, name, version, directory, license, licenseFile) {
  const files = collect(directory);
  if (licenseFile) files.push(path.resolve(directory, licenseFile));
  const records = [];
  sections.push(`\n${"=".repeat(72)}\n${ecosystem}: ${name} ${version}\nDeclared license: ${license ?? "MISSING"}\n`);
  for (const file of [...new Set(files)].sort()) {
    const bytes = readFileSync(file);
    const text = bytes.toString("utf8");
    const relative = path.relative(directory, file).split(path.sep).join("/");
    records.push({ file: relative, sha256: createHash("sha256").update(bytes).digest("hex") });
    sections.push(`\n--- ${relative} ---\n${text}\n`);
  }
  if (!records.length) sections.push("No license/notice file in this installed source package; review required.\n");
  packages.push({ ecosystem, name, version, license, files: records });
}

const members = new Set(metadata.workspace_members);
for (const pkg of metadata.packages.sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))) {
  if (!members.has(pkg.id)) add("Rust", pkg.name, pkg.version, path.dirname(pkg.manifest_path), pkg.license, pkg.license_file);
}
const lock = JSON.parse(readFileSync(path.join(root, "package-lock.json"), "utf8"));
for (const [location, pkg] of Object.entries(lock.packages).sort(([a], [b]) => a.localeCompare(b))) {
  if (!location || pkg.dev) continue;
  const directory = path.join(root, location);
  const installed = JSON.parse(readFileSync(path.join(directory, "package.json"), "utf8"));
  if (installed.version !== pkg.version) throw new Error(`Run npm ci: ${location} differs from package-lock.json`);
  add("JavaScript", installed.name, pkg.version, directory, installed.license);
}
// These assets carry notices maintained in the repository, independently of package metadata.
for (const file of ["LICENSE", ...readdirSync(path.join(root, "docs/licenses")).map((f) => `docs/licenses/${f}`)]) {
  sections.push(`\n--- inVectus: ${file} ---\n${readFileSync(path.join(root, file), "utf8")}\n`);
}
mkdirSync(output, { recursive: true });
const missing = packages.filter((pkg) => !pkg.license || !pkg.files.length);
writeFileSync(path.join(output, "THIRD_PARTY_LICENSES.txt"), sections.join("\n"));
writeFileSync(path.join(output, "inventory.json"), JSON.stringify({
  scope: "Conservative source inventory; native prebuilt runtime notices require separate review",
  packages, review_required: missing,
}, null, 2) + "\n");
console.log(`Collected ${packages.length} source packages; ${missing.length} require license/file review. Output: ${output}`);
