#!/usr/bin/env node
// Usage: node npm/publish.mjs <dist-dir>
// Publishes every platform package, then ramose last, skipping versions the registry already has.
import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";

const dist = resolve(process.argv[2] ?? "");
const dirs = readdirSync(dist).sort((a, b) => (a === "ramose") - (b === "ramose") || a.localeCompare(b));

// npm refuses to move `latest` back, and the ramose name already carries 0.3.0 from the old graph
// database, so check before any platform package goes out.
const main = JSON.parse(readFileSync(join(dist, "ramose", "package.json"), "utf8"));
const latest = spawnSync("npm", ["view", main.name, "dist-tags.latest"], { encoding: "utf8" }).stdout.trim();
const parts = (v) => v.split(/[.+-]/).slice(0, 3).map(Number);
const behind = (a, b) => parts(a).reduce((r, n, i) => r ?? (n === parts(b)[i] ? null : n < parts(b)[i]), null) ?? false;
if (latest && behind(main.version, latest)) {
  console.error(`publish: ${main.name}@${main.version} is below latest ${latest}. Bump the Cargo version past it.`);
  process.exit(1);
}

for (const dir of dirs) {
  const { name, version } = JSON.parse(readFileSync(join(dist, dir, "package.json"), "utf8"));
  const view = spawnSync("npm", ["view", `${name}@${version}`, "version"], { encoding: "utf8" });
  if (view.stdout.trim() === version) {
    console.log(`publish: ${name}@${version} already published, skipping`);
    continue;
  }
  console.log(`publish: ${name}@${version}`);
  const r = spawnSync("npm", ["publish", join(dist, dir), "--provenance", "--access", "public"], { stdio: "inherit" });
  if (r.status !== 0) {
    console.error(`publish: npm publish failed for ${name}@${version}`);
    process.exit(1);
  }
}
