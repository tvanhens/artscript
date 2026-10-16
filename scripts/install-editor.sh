#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
mkdir -p "$root/target"
(
  cd "$root/editors/vscode"
  npx --yes @vscode/vsce package --no-dependencies -o "$root/target/artscript.vsix"
)
installed=
for cli in cursor code; do
  if command -v "$cli" >/dev/null 2>&1; then
    "$cli" --install-extension "$root/target/artscript.vsix" --force
    installed="${installed:+$installed }$cli"
  fi
done
if [ -n "$installed" ]; then
  echo "installed for $installed"
fi
echo "reload the window"
