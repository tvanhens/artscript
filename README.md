<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.png">
    <img src="assets/logo-paper.png" alt="ramose" width="280">
  </picture>
</p>

<p align="center">
  A language for procedural art that a language model can write and check.<br>
  The CLI builds <code>.ramo</code> files into PNG, SVG, and GIF.
</p>

<p align="center">
  <a href="rust-toolchain.toml"><img src="https://img.shields.io/badge/rust-stable-1f2328?style=flat" alt="Stable Rust"></a>
  <a href="docs/language.md"><img src="https://img.shields.io/badge/status-v0.1%20draft-ff6b5b?style=flat" alt="v0.1 draft"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-BSL%201.1-1f2328?style=flat" alt="Business Source License 1.1"></a>
</p>

<p align="center">
  <img src="examples/spire/assets/monster/bogmaw_card.png" alt="The bogmaw: a beetle-shelled monster with three amber eyes, bone teeth, spines, moss, and glowing mushrooms on its back" width="640">
</p>

<p align="center"><sub>The bogmaw, from 199 lines of <a href="examples/spire/art/monster/bogmaw.ramo">ramose</a>.</sub></p>

ramose is a v0.1 draft. Expect breaking changes.

## Install

Install a prebuilt binary for macOS (arm64, x64), Linux glibc (x64, arm64), or Windows x64:

```sh
npm install -g ramose
```

Elsewhere, build from source with stable Rust:

```sh
cargo install --git https://github.com/tvanhens/ramose ramose
```

Make a project and build it:

```sh
mkdir my-art && cd my-art
ramose init
ramose build
```

## Learn the language

```sh
ramose docs
```

`ramose docs` prints the language card and an index of topics. `ramose docs <topic>...` prints the topics a task needs. They match the installed binary.

## Draw with a coding agent

The ramose skill teaches an agent the language and the draw, check, build, and look loop. Install the plugin for your agent.

In Codex, run:

```sh
codex plugin marketplace add tvanhens/ramose
codex plugin add ramose@ramose
```

Then ask for art in your project:

```
$ramose:ramose a 32x32 treasure chest icon with a gold trim
```

In Claude Code, run:

```
/plugin install ramose --marketplace tvanhens/ramose
```

On Claude Code before 2.1.275, run `/plugin marketplace add tvanhens/ramose`, then `/plugin install ramose@ramose`.

Then ask for art in your project:

```
/ramose a 32x32 treasure chest icon with a gold trim
```

For Cursor and other agents that read skills, run `npx skills add tvanhens/ramose`.

The agent finds or creates `ramose.toml`, writes the `.ramo` source, builds it, and looks at the PNG before it reports back. It uses `ramose` from your PATH, and runs `npx --yes ramose` when the binary is not installed.

## License

ramose is source-available under the [Business Source License 1.1](LICENSE).

- **Free.** Use ramose to make art for your own games, apps, and other projects, including commercial ones. The images and animations you build are yours, and the license places no restriction on them.
- **Needs a commercial license.** You need one to offer ramose to other people as part of a platform, such as a hosted or API asset generator, a game engine or editor that includes it, or an AI product that writes ramose for its users. [Contact the author](https://github.com/tvanhens) for terms.

Each version becomes available under the Apache License 2.0 four years after its release. [`LICENSE`](LICENSE) has the full terms.
