# npm packaging

`ramose` is a Node shim. Each `ramose-<key>` package holds one binary and declares `os`, `cpu`, and `libc`, so the package manager installs only the one that fits. There is no postinstall script, which keeps it working under bun and `--ignore-scripts`.

`platforms.json` is the only list of platforms. The shim, `stage.mjs`, and the release matrix all read it.

## One-time setup

1. Add an npm automation token as the repository secret `NPM_TOKEN`.
2. Add a fine-grained personal access token with read and write access to contents and pull requests as `RELEASE_PLEASE_TOKEN`. With `GITHUB_TOKEN`, GitHub runs no checks on the release PR.

The `ramose` name on npm first held a graph database, which published 0.2.0 and 0.3.0. `.release-please-manifest.json` starts at 0.3.0, so the first CLI release is 0.4.0 or later. `publish.mjs` stops before it publishes anything when the version is below `latest`.

## Cut a release

release-please keeps a release PR open on `main`. It reads the Conventional Commit titles since the last release, picks the next version, bumps `[workspace.package]` in `Cargo.toml`, and writes `CHANGELOG.md`. The `release` workflow then refreshes `Cargo.lock` on that PR.

Merge the release PR. release-please tags `vX.Y.Z` and creates the GitHub release. The same run builds every platform, smoke-tests each package with npm and bun, then publishes the platform packages and `ramose` last. Rerunning a half-failed release skips the versions already published.

While the version is below 1.0.0, a `feat` bumps the minor version, and a breaking change also bumps only the minor version.

## Add a platform

Add one row to `platforms.json`: `key`, `package` (`ramose-<key>`), `os`, `cpu`, `libc` for Linux, Rust `target`, GitHub `runner`, and `exe`. Nothing else lists platforms.

## Check locally

```sh
cargo build --release --locked -p ramose
node npm/stage.mjs platform darwin-arm64 target/release/ramose dist/npm
node npm/stage.mjs main dist/npm
node npm/smoke.mjs dist/npm
```
