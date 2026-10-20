#!/usr/bin/env node
// Usage: node npm/bootstrap.mjs
// Run once by hand, and again after adding a platform. npm can trust a publisher only for a package
// that exists, so this publishes a 0.0.0 placeholder of each missing package, then trusts release.yml.
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const npmDir = dirname(fileURLToPath(import.meta.url));
const platforms = JSON.parse(readFileSync(join(npmDir, "platforms.json"), "utf8"));
const repo = "tvanhens/ramose";
const names = [...platforms.map((p) => p.package), "ramose"];

function npm(args, opts = {}) {
  return spawnSync("npm", args, { stdio: "inherit", ...opts });
}

const failed = [];
for (const name of names) {
  const exists = spawnSync("npm", ["view", name, "name"], { encoding: "utf8" }).stdout.trim() === name;
  if (!exists) {
    console.log(`bootstrap: publishing placeholder ${name}@0.0.0`);
    const dir = mkdtempSync(join(tmpdir(), "ramose-placeholder-"));
    writeFileSync(
      join(dir, "package.json"),
      JSON.stringify({
        name,
        version: "0.0.0",
        description: "Placeholder. The release workflow publishes the real package.",
        license: "BUSL-1.1",
        repository: { type: "git", url: `git+https://github.com/${repo}.git` },
      }),
    );
    const r = npm(["publish", "--access", "public"], { cwd: dir });
    rmSync(dir, { recursive: true, force: true });
    if (r.status !== 0) {
      failed.push(`${name}: placeholder publish failed`);
      continue;
    }
  }

  console.log(`bootstrap: trusting ${repo} release.yml to publish ${name}`);
  const t = npm(["trust", "github", name, "--repo", repo, "--file", "release.yml", "--allow-publish", "--yes"]);
  if (t.status !== 0) {
    // npm allows one trusted publisher per package, so this also fails when one is already set.
    console.log(`bootstrap: the trusted publishers now on ${name}:`);
    npm(["trust", "list", name]);
    failed.push(`${name}: npm trust failed. If the list above already shows ${repo} release.yml, it is set.`);
  }
}

if (failed.length) {
  console.error(`\nbootstrap: needs attention:\n  ${failed.join("\n  ")}`);
  process.exit(1);
}
console.log("\nbootstrap: every package exists and trusts release.yml");
