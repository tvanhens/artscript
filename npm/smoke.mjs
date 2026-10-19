#!/usr/bin/env node
// Usage: node npm/smoke.mjs <dist-dir>
// <dist-dir> holds the staged artscript/ package and one platform package.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const win = process.platform === "win32";
const dist = resolve(process.argv[2] ?? "");
const work = mkdtempSync(join(tmpdir(), "artscript-smoke-"));

function die(step, detail) {
  console.error(`smoke: FAILED at ${step}\n${detail}`);
  process.exit(1);
}

// shell is needed on Windows for .cmd shims and npm.cmd.
function run(step, cmd, args, cwd) {
  const r = spawnSync(cmd, args, { cwd, encoding: "utf8", shell: win });
  if (r.error || r.status !== 0) {
    die(step, `${cmd} ${args.join(" ")}\n${r.error ?? ""}${r.stdout}${r.stderr}`);
  }
  return r.stdout;
}

function onPath(cmd) {
  return spawnSync(win ? "where" : "which", [cmd], { stdio: "ignore" }).status === 0;
}

const mainDir = join(dist, "artscript");
const platformKeys = readdirSync(dist).filter((d) => d !== "artscript" && existsSync(join(dist, d, "package.json")));
if (!existsSync(mainDir) || platformKeys.length !== 1) {
  die("layout", `${dist} must hold artscript/ and exactly one platform package, found: ${readdirSync(dist)}`);
}
const version = JSON.parse(readFileSync(join(mainDir, "package.json"), "utf8")).version;

const tarballs = [mainDir, join(dist, platformKeys[0])].map((dir) => {
  // npm pack prints the tarball name on its last stdout line.
  const lines = run(`pack ${dir}`, "npm", ["pack", dir, "--pack-destination", work], work).trim().split("\n");
  return join(work, lines.at(-1));
});

const installers = [
  { name: "npm", args: (tars) => ["install", "--no-audit", "--no-fund", ...tars] },
  { name: "bun", args: (tars) => ["add", ...tars] },
];

for (const { name, args } of installers) {
  if (!onPath(name)) {
    console.log(`smoke: ${name} not on PATH, skipping`);
    continue;
  }
  const project = join(work, `project-${name}`);
  mkdirSync(project);
  writeFileSync(join(project, "package.json"), '{"name":"smoke","private":true}\n');
  run(`${name} install`, name, args(tarballs), project);

  const bin = join(project, "node_modules", ".bin", win ? "artscript.cmd" : "artscript");
  const out = run(`${name} artscript --version`, bin, ["--version"], project).trim();
  if (out !== `artscript ${version}`) die(`${name} artscript --version`, `expected "artscript ${version}", got "${out}"`);

  const art = join(work, `art-${name}`);
  mkdirSync(art);
  run(`${name} artscript init`, bin, ["init"], art);
  run(`${name} artscript build`, bin, ["build"], art);
  if (!existsSync(join(art, "assets", "hello.png"))) die(`${name} artscript build`, "assets/hello.png was not written");
  console.log(`smoke: ${name} ok (${out})`);
}

rmSync(work, { recursive: true, force: true });
