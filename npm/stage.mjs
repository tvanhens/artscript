#!/usr/bin/env node
import { chmodSync, copyFileSync, cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const npmDir = dirname(fileURLToPath(import.meta.url));
const root = join(npmDir, "..");
const platforms = JSON.parse(readFileSync(join(npmDir, "platforms.json"), "utf8"));

const common = {
  license: "BUSL-1.1",
  repository: { type: "git", url: "git+https://github.com/tvanhens/artscript.git" },
};

function die(message) {
  console.error(`stage: ${message}`);
  process.exit(1);
}

function workspaceVersion() {
  const cargo = readFileSync(join(root, "Cargo.toml"), "utf8");
  const table = cargo.match(/^\[workspace\.package\][^\n]*\n((?:(?!\[)[^\n]*\n?)*)/m);
  const version = table?.[1].match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) die("no version in [workspace.package] of Cargo.toml");
  return version;
}

function writeJson(path, value) {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

function stagePlatform(version, key, binary, outDir) {
  const p = platforms.find((e) => e.key === key);
  if (!p) die(`unknown platform ${key}. Known: ${platforms.map((e) => e.key).join(", ")}`);
  const dir = join(outDir, key);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(join(dir, "bin"), { recursive: true });
  const dest = join(dir, "bin", p.exe);
  copyFileSync(binary, dest);
  chmodSync(dest, 0o755);
  writeJson(join(dir, "package.json"), {
    name: `@artscript/${key}`,
    version,
    description: `The artscript binary for ${key}.`,
    ...common,
    os: [p.os],
    cpu: [p.cpu],
    ...(p.libc && { libc: [p.libc] }),
    files: [`bin/${p.exe}`],
    preferUnplugged: true,
  });
}

function stageMain(version, outDir) {
  const dir = join(outDir, "artscript");
  rmSync(dir, { recursive: true, force: true });
  cpSync(join(npmDir, "artscript"), dir, { recursive: true });
  copyFileSync(join(npmDir, "platforms.json"), join(dir, "platforms.json"));
  for (const f of ["README.md", "LICENSE"]) copyFileSync(join(root, f), join(dir, f));
  const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
  pkg.version = version;
  pkg.optionalDependencies = Object.fromEntries(platforms.map((p) => [`@artscript/${p.key}`, version]));
  writeJson(join(dir, "package.json"), pkg);
}

const args = process.argv.slice(2);
const at = args.indexOf("--expect-version");
let expected;
if (at !== -1) {
  expected = args[at + 1];
  args.splice(at, 2);
}
const version = workspaceVersion();
if (expected !== undefined && expected !== version) {
  die(`Cargo.toml version ${version} does not match the expected ${expected}`);
}

const [command, ...rest] = args;
if (command === "platform" && rest.length === 3) {
  stagePlatform(version, rest[0], rest[1], rest[2]);
} else if (command === "main" && rest.length === 1) {
  stageMain(version, rest[0]);
} else {
  die("usage: stage.mjs [--expect-version X] platform <key> <binary> <out-dir> | main <out-dir>");
}
