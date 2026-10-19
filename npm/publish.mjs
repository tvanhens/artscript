#!/usr/bin/env node
// Usage: node npm/publish.mjs <dist-dir>
// Publishes every platform package, then artscript last, skipping versions the registry already has.
import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";

const dist = resolve(process.argv[2] ?? "");
const dirs = readdirSync(dist).sort((a, b) => (a === "artscript") - (b === "artscript") || a.localeCompare(b));

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
