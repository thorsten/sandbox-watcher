# Changelog

All notable changes to Sandbox Watcher. Generated from
[Conventional Commits](https://www.conventionalcommits.org/) by `scripts/changelog.mjs`.

## 0.2.0 (2026-10-30)

### Features

- show the product name and copyright in About and on the site ([4b0f6e7](https://github.com/thorsten/sandbox-watcher/commit/4b0f6e7393ad72b70415d4e4c7b497d8ea85cc74))
- history view, allow-list triage and cost figures in the app ([0916ce2](https://github.com/thorsten/sandbox-watcher/commit/0916ce255a6141be73ce78aa7e03edd88276e69c))
- **core:** token usage, session history and allow-list editing ([5e3012b](https://github.com/thorsten/sandbox-watcher/commit/5e3012b5bc319b6390a80607b94b58baa329e1ee))
- **scripts:** render unreleased commits under the pending version ([b9d98c0](https://github.com/thorsten/sandbox-watcher/commit/b9d98c037a5a2369bd863d91881562b72465e959))
- pick a project in the tray to select it in the window ([ae8010a](https://github.com/thorsten/sandbox-watcher/commit/ae8010aed8b4e5b2581130bf159166002f28793a))
- generate the changelog and release notes from commits ([c8dbdd3](https://github.com/thorsten/sandbox-watcher/commit/c8dbdd3661bd5f50b11d67703abe6b8e2c7e7363))

### Fixes

- **scripts:** format the changelog before the release commit ([f74d01e](https://github.com/thorsten/sandbox-watcher/commit/f74d01e8e38957d5cdfde8a9d0243aa2dc46e055))
- name app menu items and the site footer after the product ([e80d90f](https://github.com/thorsten/sandbox-watcher/commit/e80d90f0ec635c67c4ff827027f51868f948879d))
- **ui:** space the live badge from the duration in the history table ([92b046c](https://github.com/thorsten/sandbox-watcher/commit/92b046ce519822696fff06283f564ff6ca3e8883))

### CI

- build macOS bundles for Apple silicon only ([18569e1](https://github.com/thorsten/sandbox-watcher/commit/18569e1af153d28fc5ea33309ff592cafab15afb))
- build the Intel macOS bundle on Apple silicon and add a release script ([49522df](https://github.com/thorsten/sandbox-watcher/commit/49522df318a6012ce2f60fb477f089db7da9563f))

## 0.1.0 (2026-10-30)

### Features

- pick a project in the tray to select it in the window ([6adc012](https://github.com/thorsten/sandbox-watcher/commit/6adc01222c3126438bbdcc3885b724cf7537c154))
- generate the changelog and release notes from commits ([2d3fbd0](https://github.com/thorsten/sandbox-watcher/commit/2d3fbd0cf17a69194d062e689d065eee43878292))
- **site:** redesign the landing page ([3f1ce79](https://github.com/thorsten/sandbox-watcher/commit/3f1ce79ffe89bf5c673e18ccd29385adf96f24d1))
- activity timelines for Codex, Pi and OpenCode ([b883800](https://github.com/thorsten/sandbox-watcher/commit/b883800bb315e3f4306be08aaad2ebdea3ee3460))
- **ui:** master-detail layout with an Activity view ([6a44bc7](https://github.com/thorsten/sandbox-watcher/commit/6a44bc70ca65c9f381ad0a097d7bd937f89517f7))
- **core:** look inside a sandbox: processes, Claude Code transcript, git changes ([e67592e](https://github.com/thorsten/sandbox-watcher/commit/e67592ea64410997bb4bb17e0456ac40a8068d68))
- branded About panel and app menu with bundle metadata ([9ecb3c0](https://github.com/thorsten/sandbox-watcher/commit/9ecb3c06d87110c62072d35032e893f7b6bd0f7f))
- tray icon, notifications and a settings panel ([90e036a](https://github.com/thorsten/sandbox-watcher/commit/90e036a8481c93c18c0a51ffbd33744412b8d953))
- **core:** settings model and snapshot-based notification engine ([45bbc09](https://github.com/thorsten/sandbox-watcher/commit/45bbc093f925375a5c86b39e7245ceb62dfe16f7))
- open iTerm2 when installed and verify the terminal actually opened ([3816483](https://github.com/thorsten/sandbox-watcher/commit/381648392acf98f8963dfcd15dac957b696fe9b0))
- stop, open and copy actions per container and stop all ([78de1f1](https://github.com/thorsten/sandbox-watcher/commit/78de1f1280318eb4747ca214127d0eb15e4b99bf))
- support Podman and Apple container, show several runtimes at once ([e6f2659](https://github.com/thorsten/sandbox-watcher/commit/e6f265959ed80a97a0f86c1d80db0e08d5e587e6))
- add resources, logs and mounts per container ([e2bc5ed](https://github.com/thorsten/sandbox-watcher/commit/e2bc5edefe8867f39fd8eb8507998a07b2919156))
- add logo and regenerate app icons ([305b93a](https://github.com/thorsten/sandbox-watcher/commit/305b93aec094a85b8694972f27561f920071661e))
- show blocked and allowed network destinations per container ([c76cde9](https://github.com/thorsten/sandbox-watcher/commit/c76cde9900d01e906ffac14aa5c9218b7b0caac2))
- show sessions per container and refresh on runtime events ([d92794e](https://github.com/thorsten/sandbox-watcher/commit/d92794ea6d45eff46ea085ebc133967c95595c53))
- **core:** add Docker adapter and live runtime resolution ([c7d405e](https://github.com/thorsten/sandbox-watcher/commit/c7d405efee8701bcf876a9a0d99bdbf1b5a4cb93))
- scaffold Tauri app, Rust core crate, web UI and CI ([922911a](https://github.com/thorsten/sandbox-watcher/commit/922911a2b39f7c1b6a081621e3c710fe55c74760))

### Fixes

- **site:** avoid a 404 in the console before the first release ([b7d4413](https://github.com/thorsten/sandbox-watcher/commit/b7d44138c7cd9fb18db1ec539420adc942fe5d23))
- **core:** send the key-only label filter to Docker only ([f66726f](https://github.com/thorsten/sandbox-watcher/commit/f66726f010050ada5ef0dc9ad380c0e3ab4c58ce))
- **core:** never miss container events emitted before the stream is polled ([66de7ff](https://github.com/thorsten/sandbox-watcher/commit/66de7ff62d85c91a445c324a5fe6f28b8352a3fd))
- **core:** find the project mount the way sandbox mounts it ([2c2bb53](https://github.com/thorsten/sandbox-watcher/commit/2c2bb53d5c3bbc8e0484b0141d23e877009d0ffe))
- **core:** poll when a runtime has no event backend, accept Podman event names ([76963ca](https://github.com/thorsten/sandbox-watcher/commit/76963ca3eebfebed20e3a5f17956325c6066eab6))
- **ui:** write to the clipboard once per copy action ([8876501](https://github.com/thorsten/sandbox-watcher/commit/88765014a52bfd3b9b7131940beb5703603e3dc1))
- **core:** expose the cached adapter on LiveRuntime ([315883b](https://github.com/thorsten/sandbox-watcher/commit/315883b79e65a10ca68226cd1186be4b00aa0c7f))
- **core:** gate unix socket connection so the crate builds on Windows ([bcc08b4](https://github.com/thorsten/sandbox-watcher/commit/bcc08b43a9beaf8c7660fdf9ca34d01ec80ec364))

### Documentation

- add AGENTS.md with rules, commands and verification expectations ([10dd283](https://github.com/thorsten/sandbox-watcher/commit/10dd283bf5bd235249944c26de09e63eb7ba58f1))
- add milestone plan for Sandbox Watcher ([296512b](https://github.com/thorsten/sandbox-watcher/commit/296512b263dbf04d7229ee5616b3f077a1b93bc1))

### Tests

- **core:** make event forwarding test independent of task timing ([1601f99](https://github.com/thorsten/sandbox-watcher/commit/1601f990de1e98aaf924d9cb7543d33fb5c7f16a))

### Build

- install native binaries for macOS, Linux and Windows ([8e01cb8](https://github.com/thorsten/sandbox-watcher/commit/8e01cb85424c0a1feee0d2fecfb50dcf9c04fc15))
- replace Biome with oxlint and oxfmt, add MIT license ([cb46c38](https://github.com/thorsten/sandbox-watcher/commit/cb46c3836e07c4d94b17f1306d7443859cc7fd30))

### CI

- let the Pages workflow enable Pages on first run ([807412a](https://github.com/thorsten/sandbox-watcher/commit/807412a4d5dc857c358b1b81a646796f7f70709c))
- dump Bodman compat API responses when the conformance test fails ([293cdaf](https://github.com/thorsten/sandbox-watcher/commit/293cdafdbcfb7ed293d77a1fd68573c9b8c231a3))
- publish releases for all platforms and deploy a GitHub Pages site ([69f829d](https://github.com/thorsten/sandbox-watcher/commit/69f829dfa955f972ab47b274ca8a0403a3bb062f))
- run Docker conformance test and build installers with tauri-action ([0708724](https://github.com/thorsten/sandbox-watcher/commit/07087240bd2b980d8180a9ee4f1344b5e2a9a6a2))

### Chores

- keep version at 0.1.0 until the first release is cut ([167cca6](https://github.com/thorsten/sandbox-watcher/commit/167cca69c0e1b0205e07327ff24f3f5f59490259))
- **release:** version 1.0.0 ([6c16d43](https://github.com/thorsten/sandbox-watcher/commit/6c16d4327a1ff14a58dff3384fd3fcab8c97da5a))
- add commitlint and husky hooks ([795d07a](https://github.com/thorsten/sandbox-watcher/commit/795d07a6d495dec57e56d6207e64cd9e94b1f7f9))
