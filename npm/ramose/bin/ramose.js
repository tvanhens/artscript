#!/usr/bin/env node
"use strict";

const { spawnSync } = require("node:child_process");
const platforms = require("../platforms.json");

function fail(message) {
  console.error(`ramose: ${message}`);
  process.exit(1);
}

function isMusl() {
  if (process.platform !== "linux") return false;
  return !process.report.getReport().header.glibcVersionRuntime;
}

const musl = isMusl();
const here = `${process.platform}-${process.arch}${musl ? " (musl)" : ""}`;
const supported = platforms.map((p) => p.key).join(", ");
const fallback = "cargo install --git https://github.com/tvanhens/ramose ramose";

const entry = musl ? undefined : platforms.find((p) => p.os === process.platform && p.cpu === process.arch);
if (!entry) {
  fail(`no prebuilt binary for ${here}. Supported: ${supported}. Build from source with: ${fallback}`);
}

let binary;
try {
  binary = require.resolve(`${entry.package}/bin/${entry.exe}`);
} catch {
  fail(
    `the package ${entry.package} is not installed for ${here}. ` +
      `Reinstall without --no-optional or --omit=optional. Supported: ${supported}. ` +
      `Or build from source with: ${fallback}`,
  );
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) fail(`could not run ${binary}: ${result.error.message}`);
if (result.signal) {
  process.kill(process.pid, result.signal);
  // Reached only when the signal is ignored or handled.
  process.exit(1);
}
process.exit(result.status);
