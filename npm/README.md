# npm packaging

`artscript` is a Node shim. Each `artscript-<key>` package holds one binary and declares `os`, `cpu`, and `libc`, so the package manager installs only the one that fits. There is no postinstall script, which keeps it working under bun and `--ignore-scripts`.

`platforms.json` is the only list of platforms. The shim, `stage.mjs`, and the release matrix all read it.

## One-time setup

Add an automation token as the repository secret `NPM_TOKEN`.

## Cut a release

1. Bump `version` in `[workspace.package]` of the root `Cargo.toml` and merge.
2. Tag the merge commit `vX.Y.Z` and push the tag.

The `release` workflow builds every platform, smoke-tests each package with npm and bun, then publishes the platform packages and `artscript` last. It fails if the tag differs from the Cargo version. Rerunning a half-failed release skips the versions already published.

## Add a platform

Add one row to `platforms.json`: `key`, `package` (`artscript-<key>`), `os`, `cpu`, `libc` for Linux, Rust `target`, GitHub `runner`, and `exe`. Nothing else lists platforms.

## Check locally

```sh
cargo build --release --locked -p artscript
node npm/stage.mjs platform darwin-arm64 target/release/artscript dist/npm
node npm/stage.mjs main dist/npm
node npm/smoke.mjs dist/npm
```
