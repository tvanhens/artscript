#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
mkdir -p "$root/target"
(
  cd "$root/editors/vscode"
  npx --yes @vscode/vsce package --no-dependencies -o "$root/target/artscript.vsix"
)

cursor_cli() {
  for candidate in \
    "/Applications/Cursor.app/Contents/Resources/app/bin/cursor" \
    "$HOME/Applications/Cursor.app/Contents/Resources/app/bin/cursor"
  do
    if [ -x "$candidate" ]; then
      echo "$candidate"
      return 0
    fi
  done
  if command -v cursor >/dev/null 2>&1; then
    command -v cursor
    return 0
  fi
  return 1
}

installed=
if cli=$(cursor_cli); then
  "$cli" --install-extension "$root/target/artscript.vsix" --force
  installed="cursor"
fi
if command -v code >/dev/null 2>&1; then
  code --install-extension "$root/target/artscript.vsix" --force
  installed="${installed:+$installed }code"
fi
if [ -n "$installed" ]; then
  echo "installed for $installed"
fi
echo "reload the window"
