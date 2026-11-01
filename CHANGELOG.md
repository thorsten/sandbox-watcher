# Changelog

All notable changes to Sandbox Watcher. Generated from
[Conventional Commits](https://www.conventionalcommits.org/) by `scripts/changelog.mjs`.

## 0.7.0 (2026-11-01)

### Features

- **ui:** import ssh docker contexts as remote hosts ([1101675](https://github.com/thorsten/sandbox-watcher/commit/1101675774a4272afe58ae95efc98b2f587bc382))
- command to list ssh docker contexts ([0eae6c6](https://github.com/thorsten/sandbox-watcher/commit/0eae6c682f99afb2701147e4e76ea6a759650194))
- **core:** list ssh docker contexts for import ([6723722](https://github.com/thorsten/sandbox-watcher/commit/6723722f74e79aad451f8c00c9133c92a1108ac4))

### Fixes

- **contracts:** sort keys in the shell fixtures ([920fa20](https://github.com/thorsten/sandbox-watcher/commit/920fa209b800621c7b6b5a8fa606dfd304896ae6))
- **ui:** poll resources and history through usePolling ([5ce7734](https://github.com/thorsten/sandbox-watcher/commit/5ce7734f27933e62f6048a0d1d43c46fa4871911))
- **core:** allow list test tolerates the Windows lock file ([f72193f](https://github.com/thorsten/sandbox-watcher/commit/f72193fea49a4f64ab666a00b91ebc40f2e9a1b0))
- **ui:** poll activity and logs one request at a time ([ffea201](https://github.com/thorsten/sandbox-watcher/commit/ffea2012740d18d313aa460b1eddfe6dc248362a))
- **core:** attribute transcripts to sessions and read them incrementally ([fa345c8](https://github.com/thorsten/sandbox-watcher/commit/fa345c88c9fdb536963de13beb25163a3276dd97))
- **core:** harden the allow list write ([95cff8f](https://github.com/thorsten/sandbox-watcher/commit/95cff8fcf1b418b8781b8fb9cf21f41b06894ec3))
- **ui:** show the ssh config entry and report copy result ([4d14d0d](https://github.com/thorsten/sandbox-watcher/commit/4d14d0d98aa7b846eb7607c8115155b2f768bdce))
- **core:** no ssh config snippet for endpoints with whitespace ([e72f1a4](https://github.com/thorsten/sandbox-watcher/commit/e72f1a4661ffbc8b3870a402d0c9bbf70a864381))
- **core:** keep demo contexts above the mock tests ([a1717eb](https://github.com/thorsten/sandbox-watcher/commit/a1717eb0acb1a4624b0f58152bf304578309ac38))

### Documentation

- importing ssh docker contexts ([003d584](https://github.com/thorsten/sandbox-watcher/commit/003d584c03508b16b506f565ed11fd89be2846e7))
- implementation plan for importing ssh docker contexts ([0050fb0](https://github.com/thorsten/sandbox-watcher/commit/0050fb0787a28370c06ecab7a4c9ef3c7ec61983))
- design for importing ssh docker contexts ([446bc1c](https://github.com/thorsten/sandbox-watcher/commit/446bc1c350ee12cba923109be214b2ae95e974b2))
- mark remote hosts shipped, plan 0.7.0 follow-ups ([e1d7473](https://github.com/thorsten/sandbox-watcher/commit/e1d7473837fb2ea2166ceb8dd7ba57e55d776d39))
- **site:** link the security policy in the footer ([50f4f6e](https://github.com/thorsten/sandbox-watcher/commit/50f4f6e563e0458509c06a13510be37fe047194f))
- add code of conduct, contributing guide and security policy ([e3f3d47](https://github.com/thorsten/sandbox-watcher/commit/e3f3d47b6d783f81747599aec7282dc130f91ee1))

### Build

- **deps:** upgrade TypeScript to 7.0.2 ([1dd33b1](https://github.com/thorsten/sandbox-watcher/commit/1dd33b13f3060301f2cbeca4cbac62666e2b9358))
- bump @types/node from 24.19.2 to 26.6.6 ([635354f](https://github.com/thorsten/sandbox-watcher/commit/635354fb1569969329bf496d396b26a8c2e5c0ab))

### CI

- bump the actions group across 1 directory with 8 updates ([b711a22](https://github.com/thorsten/sandbox-watcher/commit/b711a22bbd06bfd546e77218ecdfcbbb516edb31))
- check out text files with LF on every platform ([7d0ebf7](https://github.com/thorsten/sandbox-watcher/commit/7d0ebf7ef98ba5c09a1c101c50ea087dc2d5705e))
- add contract, audit, pin and performance checks ([6c56c4d](https://github.com/thorsten/sandbox-watcher/commit/6c56c4dd102b737367f65e05730bf078aee0d04f))
- generate a cyclonedx sbom on every build and release ([40cf99b](https://github.com/thorsten/sandbox-watcher/commit/40cf99b68b946786b204dbdecfd183bc46f75185))

## 0.6.0 (2026-10-31)

### Features

- **ui:** remote host badges, filter, settings and mock host ([c6816fc](https://github.com/thorsten/sandbox-watcher/commit/c6816fc8605a462d7c1d1570f63ba65af1a7a629))
- connect to remote hosts from settings ([7926c8e](https://github.com/thorsten/sandbox-watcher/commit/7926c8e6c5cd31fd1fb0845a24cb3bbd300a6c9a))
- **core:** detect runtimes on remote hosts ([5ac946d](https://github.com/thorsten/sandbox-watcher/commit/5ac946d4375f589611de0e193b80a4a0f47643fc))
- **core:** ssh master sessions with socket forwarding ([2cdc265](https://github.com/thorsten/sandbox-watcher/commit/2cdc265bb5a5c3b39eeee8193699b80ea0a9cf98))
- **core:** host identity and container keys ([15b925c](https://github.com/thorsten/sandbox-watcher/commit/15b925cb4cd5380f1eadfca48a1e62a55548e463))

### Fixes

- **ui:** show greyed containers as unreachable and name remote runtimes ([34f4060](https://github.com/thorsten/sandbox-watcher/commit/34f4060d3710edf0d6d0f7848646f10522f832fc))
- **core:** retry a failed remote runtime after the host backoff ([7b7d668](https://github.com/thorsten/sandbox-watcher/commit/7b7d668a4009661970c50b373b553df4620a58d9))
- bound remote round trips after the overview ([e9d5939](https://github.com/thorsten/sandbox-watcher/commit/e9d593989e33d8e95b5f4415ff852ddf8d68d59f))
- **core:** harden ssh options and report failed remote probes ([62ac148](https://github.com/thorsten/sandbox-watcher/commit/62ac1483653f59a5be6c75df4c4b0b5eb066b566))
- **ui:** keep host filter reachable and skip greyed sessions in counts ([ad30011](https://github.com/thorsten/sandbox-watcher/commit/ad30011036d9bd33e90da22113159f9726fb9085))
- **core:** keep history and shutdown correct for unreachable hosts ([5261b2f](https://github.com/thorsten/sandbox-watcher/commit/5261b2f2361f605a113092980f4e0b975c56a4d9))
- **core:** drain ssh master stderr and validate forwards ([575b578](https://github.com/thorsten/sandbox-watcher/commit/575b578de0457225d22cd23ffa5063a04c6c12f5))

### Refactoring

- **core:** run runtime CLIs through a CommandRunner ([6cceec6](https://github.com/thorsten/sandbox-watcher/commit/6cceec6084a6aa4f88dc3fb44ecb76e77fa1f8ef))
- address containers by container key ([ee1b650](https://github.com/thorsten/sandbox-watcher/commit/ee1b650493f25b89e682b570fc49728391db0bc3))
- stop all covers every host ([bf7d551](https://github.com/thorsten/sandbox-watcher/commit/bf7d551721073696f70459998d24ba00070e0954))
- **core:** carry host and container key through the overview ([49f44f1](https://github.com/thorsten/sandbox-watcher/commit/49f44f166ef904e455b768b09a499661eaa6fa6b))

### Documentation

- implementation plan for remote hosts ([88d8372](https://github.com/thorsten/sandbox-watcher/commit/88d8372e12616032fadbc8f267465240bcd9c079))
- design for remote hosts over SSH ([0f275b4](https://github.com/thorsten/sandbox-watcher/commit/0f275b472f1a7dc78faa1cf015d0ce720cae4f36))
- mark Apple container and fresh-machine checks as verified ([f6592d1](https://github.com/thorsten/sandbox-watcher/commit/f6592d168b8b351109a212c6869c38b915abf874))

### Tests

- run conformance over SSH in CI; document remote hosts ([d8db92f](https://github.com/thorsten/sandbox-watcher/commit/d8db92f5283a94ea04e47fd0e123a7b78dc4d0fd))

### CI

- restart test containers before the ssh conformance runs ([584039e](https://github.com/thorsten/sandbox-watcher/commit/584039e5a2efc5a43f3ccac176505330ce4a1c08))
- run the shell unit tests; mark remote hosts in progress ([2db7ea5](https://github.com/thorsten/sandbox-watcher/commit/2db7ea585510e9037ea5240de0f27aecce927feb))
- weekly check of the sandbox files the watcher depends on ([ad25800](https://github.com/thorsten/sandbox-watcher/commit/ad25800e1811be238fedba9818c20ea7d660cc39))

### Chores

- **release:** v0.6.0 ([0652349](https://github.com/thorsten/sandbox-watcher/commit/0652349b90c9ecad2f425d07be35c33eec84d411))

**Full changelog:** [v0.5.0...v0.6.0](https://github.com/thorsten/sandbox-watcher/compare/v0.5.0...v0.6.0)

## 0.5.0 (2026-10-31)

### Features

- **core:** prices for Claude 5 models and current Opus 4.x ([bc32e9b](https://github.com/thorsten/sandbox-watcher/commit/bc32e9bf7871b78b0fedfdf05108ed0a5b329b7e))
- turn-level detail per agent session ([38f855a](https://github.com/thorsten/sandbox-watcher/commit/38f855a64abba07e91dd716152575b4e2be2aef3))

### Fixes

- find runtimes when started from Finder or the Dock ([0d9cd9f](https://github.com/thorsten/sandbox-watcher/commit/0d9cd9f66a58d1edebfe8536c174f25ed6ae5b9e))
- **core:** count Claude Code tokens once per API message ([84083c7](https://github.com/thorsten/sandbox-watcher/commit/84083c7352e7abfd1815fbe4841a2838d2f26798))

### Documentation

- mention the update check in the README privacy section ([61505e6](https://github.com/thorsten/sandbox-watcher/commit/61505e62c6180c69c70f3ec1e54494d287e8e2f4))
- macOS builds are signed and notarised ([d1df000](https://github.com/thorsten/sandbox-watcher/commit/d1df0005a2e221ee608b578ffdbd1194fe41e6af))
- add troubleshooting guide ([b10f3c7](https://github.com/thorsten/sandbox-watcher/commit/b10f3c752efc26ceceac7fd5b028113d3cc4bbec))
- **site:** fill in address in privacy policy ([024f5a7](https://github.com/thorsten/sandbox-watcher/commit/024f5a779012639b121120aa4d5e4274fb388056))
- **site:** add imprint and privacy policy pages ([d5dd82c](https://github.com/thorsten/sandbox-watcher/commit/d5dd82c8b7aa4e110c5d0c9c097af1b944fe86cc))

### CI

- **release:** sign and notarise macOS builds when Apple secrets exist ([f2653aa](https://github.com/thorsten/sandbox-watcher/commit/f2653aadc6be5e0f9692a409ef599dcacab9eea0))

### Chores

- **release:** v0.5.0 ([6a59d7e](https://github.com/thorsten/sandbox-watcher/commit/6a59d7eab17ac03f09419b6eb91bf46bb1ee7075))
- ignore signing certificates and keys ([c84f4e1](https://github.com/thorsten/sandbox-watcher/commit/c84f4e152966cf3cf90686744778620c5f4e15c7))
- **release:** bump the README status line with the version ([4c5d78a](https://github.com/thorsten/sandbox-watcher/commit/4c5d78a17228ea869819e5b22ff99aff1e3ac389))
- ignore personal Claude Code settings ([9260382](https://github.com/thorsten/sandbox-watcher/commit/92603829fd96a550fbfe018da7865911c1c4f1d9))
- add .editorconfig with spaces-only indentation ([86184c0](https://github.com/thorsten/sandbox-watcher/commit/86184c0e3dc4153486b2ef032864aabbbe233137))

**Full changelog:** [v0.4.0...v0.5.0](https://github.com/thorsten/sandbox-watcher/compare/v0.4.0...v0.5.0)

## 0.4.0 (2026-10-30)

### Features

- show touched files and open them in your editor ([9da680b](https://github.com/thorsten/sandbox-watcher/commit/9da680bda037f20be208204d39b6a811ef3b22bc))
- **core:** files per agent session and editor launching ([27150f5](https://github.com/thorsten/sandbox-watcher/commit/27150f5aebe01dce047a8ac9edc92aec27fbffee))

### Chores

- **release:** v0.4.0 ([b761ce0](https://github.com/thorsten/sandbox-watcher/commit/b761ce02b2e65d9823cca9ccdba869705101b096))

**Full changelog:** [v0.3.0...v0.4.0](https://github.com/thorsten/sandbox-watcher/compare/v0.3.0...v0.4.0)

## 0.3.0 (2026-10-30)

### Features

- self-update from GitHub Releases ([2267c8e](https://github.com/thorsten/sandbox-watcher/commit/2267c8ea4d8bda026b8d81cc2da55e02cf6f83c8))

### Tests

- **ui:** cover every component and helper with Vitest ([45266bb](https://github.com/thorsten/sandbox-watcher/commit/45266bbe0b5a6e70025271cee420dca7f99f133e))

### Chores

- **release:** v0.3.0 ([5c81601](https://github.com/thorsten/sandbox-watcher/commit/5c8160132dba98c0ad85e02cd7c71f635fa8f94d))
- pin pnpm 11.22.0 and stop the release script from reinstalling ([33d0e35](https://github.com/thorsten/sandbox-watcher/commit/33d0e3578305e10d4c1d2d81bf8b944c3bd67d85))

**Full changelog:** [v0.2.0...v0.3.0](https://github.com/thorsten/sandbox-watcher/compare/v0.2.0...v0.3.0)

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

### Chores

- **release:** v0.2.0 ([b2efe21](https://github.com/thorsten/sandbox-watcher/commit/b2efe218d2e851e5cd06747a55be60edaf7d6181))

**Full changelog:** [v0.1.0...v0.2.0](https://github.com/thorsten/sandbox-watcher/compare/v0.1.0...v0.2.0)

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
