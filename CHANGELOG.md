# Changelog

## [0.5.0](https://github.com/tvanhens/ramose/compare/v0.4.0...v0.5.0) (2026-10-23)


### ⚠ BREAKING CHANGES

* **eval:** derive parameter types from the default's syntax ([#345](https://github.com/tvanhens/ramose/issues/345))
* **eval:** report time outside an anim patch value as E0345 ([#346](https://github.com/tvanhens/ramose/issues/346))
* **eval:** every entry file needs `@artboard`. `viewport` moves from `@target` to `@artboard`. Reading `canvas` is E0320.
* **eval:** rename z: to order: ([#333](https://github.com/tvanhens/ramose/issues/333))

### Features

* **bench:** add wow.py, a first-shot quality eval for the ramose skill ([#271](https://github.com/tvanhens/ramose/issues/271)) ([1e150f4](https://github.com/tvanhens/ramose/commit/1e150f4ef058000cb8770b6000daa6ee4cf5b75d))
* **bench:** judge wow.py with opus and sonnet only ([#274](https://github.com/tvanhens/ramose/issues/274)) ([ed5906c](https://github.com/tvanhens/ramose/commit/ed5906c094aa24fb34be3e849badae1f9510959c))
* **bench:** judge wow.py within each image's style ([#275](https://github.com/tvanhens/ramose/issues/275)) ([b94d777](https://github.com/tvanhens/ramose/commit/b94d777948ef8b94d4a8a1323f717feae21772eb))
* **bench:** make Codex quality comparisons reproducible ([#281](https://github.com/tvanhens/ramose/issues/281)) ([65e3cb7](https://github.com/tvanhens/ramose/commit/65e3cb7b86dd583b521abe14e86b73a12339a908))
* **eval:** accept three-component vectors and sizes ([#341](https://github.com/tvanhens/ramose/issues/341)) ([eadae83](https://github.com/tvanhens/ramose/commit/eadae8354fb9f1b69873cc01f8f43f1b61907a22))
* **eval:** add a path value with sampling functions ([#351](https://github.com/tvanhens/ramose/issues/351)) ([7c440af](https://github.com/tvanhens/ramose/commit/7c440aff14f252416a19f2e6430412a2dfe3e9df))
* **eval:** declare the artboard once and frame each output into it ([#344](https://github.com/tvanhens/ramose/issues/344)) ([585bd46](https://github.com/tvanhens/ramose/commit/585bd468a25fb0cb31a5def502665db1c503b164))
* **eval:** derive parameter types from the default's syntax ([#345](https://github.com/tvanhens/ramose/issues/345)) ([ae4cadc](https://github.com/tvanhens/ramose/commit/ae4cadc91e53b3faa371de775f9f6481bbe1198c))
* **eval:** rename z: to order: ([#333](https://github.com/tvanhens/ramose/issues/333)) ([1c0f7f0](https://github.com/tvanhens/ramose/commit/1c0f7f02a78a5c0b3448908c41ca54c2df80f0dd))
* **eval:** report time outside an anim patch value as E0345 ([#346](https://github.com/tvanhens/ramose/issues/346)) ([a54c4f8](https://github.com/tvanhens/ramose/commit/a54c4f8a77a8de90fa85e9950cd995203da1588c)), closes [#289](https://github.com/tvanhens/ramose/issues/289)
* **plugin:** install the ramose skill as a Claude Code plugin ([#268](https://github.com/tvanhens/ramose/issues/268)) ([f49e1d9](https://github.com/tvanhens/ramose/commit/f49e1d9f884e4ad29385685ab9c8f620bc8da4aa))
* **plugin:** support Codex in the shared ramose skill ([#277](https://github.com/tvanhens/ramose/issues/277)) ([4dd3f1f](https://github.com/tvanhens/ramose/commit/4dd3f1f6f37a6ecd722ceebffe38d79e0adfa325))
* **preview:** add entry parameters and visual inspection CLI ([#283](https://github.com/tvanhens/ramose/issues/283)) ([2575c4a](https://github.com/tvanhens/ramose/commit/2575c4a727836d22e78c9a40be13f4797335cdbb))
* **skill:** require targeted preview self-review ([#284](https://github.com/tvanhens/ramose/issues/284)) ([b7aaa68](https://github.com/tvanhens/ramose/commit/b7aaa6868385697083026c5fed69a068f7285f8d))
* **std:** add brush form, a lit-volume shade for fills ([#273](https://github.com/tvanhens/ramose/issues/273)) ([bb8030d](https://github.com/tvanhens/ramose/commit/bb8030d766aa91f0693a3d16304c152ab6ec434c))
* **warp:** map a node through corners and cylinder warps ([#270](https://github.com/tvanhens/ramose/issues/270)) ([026ab99](https://github.com/tvanhens/ramose/commit/026ab99ccc4d30eb5e2edf0d0a1d645ad4d4a2ea))


### Bug Fixes

* **eval:** report a play reached after t=0 that outlasts the clip ([#337](https://github.com/tvanhens/ramose/issues/337)) ([3919782](https://github.com/tvanhens/ramose/commit/391978241b7dd381a5341c010cda07ab62f3fa6e))
* **eval:** report a rejected --set once and stop the entry ([#286](https://github.com/tvanhens/ramose/issues/286)) ([91cdcd0](https://github.com/tvanhens/ramose/commit/91cdcd030b37b03a044d239f28b2bb4b48171c3e)), closes [#282](https://github.com/tvanhens/ramose/issues/282)
* **eval:** seed a seeded instance from its def's name, not its module path ([#332](https://github.com/tvanhens/ramose/issues/332)) ([ce55755](https://github.com/tvanhens/ramose/commit/ce55755d6cfc4772b22534125680ad6326098f65))

## [0.4.0](https://github.com/tvanhens/ramose/compare/v0.3.0...v0.4.0) (2026-10-20)


### Features

* **brush:** let a brush read the backdrop as m.below ([#265](https://github.com/tvanhens/ramose/issues/265)) ([599a331](https://github.com/tvanhens/ramose/commit/599a33177ddd352342cd020b034a610230c70ea6))
* **eval:** scope names by shape and spell the full shape path ([#263](https://github.com/tvanhens/ramose/issues/263)) ([e2f4c89](https://github.com/tvanhens/ramose/commit/e2f4c8952974e6d02476171ef63c7921c089ae4a))
* **npm:** install ramose with npm or bun ([#229](https://github.com/tvanhens/ramose/issues/229)) ([be6c057](https://github.com/tvanhens/ramose/commit/be6c0570e2cd8efaea1ecf17fa94581b3a09e06e))
