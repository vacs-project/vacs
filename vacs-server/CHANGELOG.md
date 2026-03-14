# Changelog

## [2.1.0](https://github.com/vacs-project/vacs/compare/vacs-server-v2.0.0...vacs-server-v2.1.0) (2026-03-14)


### Features

* **vacs-server:** add coverage debug endpoints ([#623](https://github.com/vacs-project/vacs/issues/623)) ([b609bb0](https://github.com/vacs-project/vacs/commit/b609bb0dd95a54d5e8883466e2281cd50022156e))
* **vacs-server:** add token-based authentication ([#615](https://github.com/vacs-project/vacs/issues/615)) ([f79ee01](https://github.com/vacs-project/vacs/commit/f79ee01b6d8b56bf7ed270ff5d9b4f888538527a))
* **vacs-server:** extend metrics to include call routing and coverage ([#624](https://github.com/vacs-project/vacs/issues/624)) ([e60a0a3](https://github.com/vacs-project/vacs/commit/e60a0a3a3cb5f029dfb6e2a0daf68e28ba28e424))


### Bug Fixes

* **vacs-server:** correct station handoff handling in coverage state sync ([#630](https://github.com/vacs-project/vacs/issues/630)) ([5ed1aae](https://github.com/vacs-project/vacs/commit/5ed1aae0782454f001e2374c29a3c8de92442f99))

## [2.0.0](https://github.com/vacs-project/vacs/compare/vacs-server-v1.1.1...vacs-server-v2.0.0) (2026-03-01)


### ⚠ BREAKING CHANGES

* implement station coverage calculations and calling ([#452](https://github.com/vacs-project/vacs/issues/452))
* overhaul UI with geo/tabbed layout and station-based calling ([#531](https://github.com/vacs-project/vacs/issues/531))

### Features

* add priority calls ([#504](https://github.com/vacs-project/vacs/issues/504)) ([384131b](https://github.com/vacs-project/vacs/commit/384131bf18dbe8240602d6f4e0b226fb04effdf3))
* implement station coverage calculations and calling ([#452](https://github.com/vacs-project/vacs/issues/452)) ([384131b](https://github.com/vacs-project/vacs/commit/384131bf18dbe8240602d6f4e0b226fb04effdf3))
* overhaul UI with geo/tabbed layout and station-based calling ([#531](https://github.com/vacs-project/vacs/issues/531)) ([384131b](https://github.com/vacs-project/vacs/commit/384131bf18dbe8240602d6f4e0b226fb04effdf3))
* **vacs-client:** add call start and end sounds ([#505](https://github.com/vacs-project/vacs/issues/505)) ([384131b](https://github.com/vacs-project/vacs/commit/384131bf18dbe8240602d6f4e0b226fb04effdf3))
* **vacs-client:** add keybind for toggling radio prio ([#500](https://github.com/vacs-project/vacs/issues/500)) ([384131b](https://github.com/vacs-project/vacs/commit/384131bf18dbe8240602d6f4e0b226fb04effdf3))
* **vacs-client:** add window zoom hotkeys ([#522](https://github.com/vacs-project/vacs/issues/522)) ([384131b](https://github.com/vacs-project/vacs/commit/384131bf18dbe8240602d6f4e0b226fb04effdf3))
* **vacs-client:** implement telephone directory ([#490](https://github.com/vacs-project/vacs/issues/490)) ([384131b](https://github.com/vacs-project/vacs/commit/384131bf18dbe8240602d6f4e0b226fb04effdf3))
* **vacs-server:** implement dataset reloading ([#555](https://github.com/vacs-project/vacs/issues/555)) ([5652687](https://github.com/vacs-project/vacs/commit/5652687fadc439188b67554b1a3d722ae320f1f8))


### Bug Fixes

* **vacs-client:** fix error while switching to exclusive audio device ([#498](https://github.com/vacs-project/vacs/issues/498)) ([384131b](https://github.com/vacs-project/vacs/commit/384131bf18dbe8240602d6f4e0b226fb04effdf3))
* **vacs-client:** prevent call queue from shrinking ([384131b](https://github.com/vacs-project/vacs/commit/384131bf18dbe8240602d6f4e0b226fb04effdf3))
* **vacs-server:** fix coverage calculations for VATSIM-only positions ([#550](https://github.com/vacs-project/vacs/issues/550)) ([5276570](https://github.com/vacs-project/vacs/commit/52765707c9a82b373affc5371dda7ef4ab2f7977))
* **vacs-server:** ignore datafeed SUP connection ([#480](https://github.com/vacs-project/vacs/issues/480)) ([384131b](https://github.com/vacs-project/vacs/commit/384131bf18dbe8240602d6f4e0b226fb04effdf3))
* **vacs-server:** resolve annotated dataset tag to commit SHA ([#571](https://github.com/vacs-project/vacs/issues/571)) ([109b34d](https://github.com/vacs-project/vacs/commit/109b34d203182f78b59d1a0ab306235c10949295))

## [1.1.1](https://github.com/vacs-project/vacs/compare/vacs-server-v1.1.0...vacs-server-v1.1.1) (2025-12-05)

## [1.1.0](https://github.com/vacs-project/vacs/compare/vacs-server-v1.0.0...vacs-server-v1.1.0) (2025-11-30)

### Features

- provide TURN servers for traversing restrictive networks ([#248](https://github.com/vacs-project/vacs/issues/248)) ([e4b8b91](https://github.com/vacs-project/vacs/commit/e4b8b91320fd6d072ef4ba1c98de56ad14c8dcfe))
- **vacs-client:** add profile select to mission page ([ad36dc5](https://github.com/vacs-project/vacs/commit/ad36dc55e2e42619eff9c0163e869f64910998bb))
- **vacs-client:** add station filter and aliasing ([#233](https://github.com/vacs-project/vacs/issues/233)) ([ad36dc5](https://github.com/vacs-project/vacs/commit/ad36dc55e2e42619eff9c0163e869f64910998bb))
- **vacs-client:** load ICE config after signaling connect ([e4b8b91](https://github.com/vacs-project/vacs/commit/e4b8b91320fd6d072ef4ba1c98de56ad14c8dcfe))
- **vacs-server:** implement GitHub release catalog ([#258](https://github.com/vacs-project/vacs/issues/258)) ([6dac184](https://github.com/vacs-project/vacs/commit/6dac18498899760e654fe7485bce4944a8a723ac))
- **vacs-server:** implement Prometheus metrics ([#251](https://github.com/vacs-project/vacs/issues/251)) ([b6d72fd](https://github.com/vacs-project/vacs/commit/b6d72fd6bfa719380efa966d55c02b85800978f6))
- **vacs-webrtc:** use shared IceConfig types ([e4b8b91](https://github.com/vacs-project/vacs/commit/e4b8b91320fd6d072ef4ba1c98de56ad14c8dcfe))

### Bug Fixes

- **vacs-client:** remove spammy updater progress log ([6dac184](https://github.com/vacs-project/vacs/commit/6dac18498899760e654fe7485bce4944a8a723ac))
- **vacs-server:** prevent clients from sending signaling messages to own peer_id ([#244](https://github.com/vacs-project/vacs/issues/244)) ([098ec4c](https://github.com/vacs-project/vacs/commit/098ec4cd0d79225b8542710199f79f3e9e84dac0))

## [1.0.0](https://github.com/vacs-project/vacs/compare/vacs-server-v0.2.0...vacs-server-v1.0.0) (2025-11-14)

### Bug Fixes

- **vacs-client:** fix DA key overflow and sorting ([#204](https://github.com/vacs-project/vacs/issues/204)) ([c1b2da5](https://github.com/vacs-project/vacs/commit/c1b2da5e39126b033fa24251eb725001c244080a))

## [0.2.0](https://github.com/vacs-project/vacs/compare/vacs-server-v0.1.0...vacs-server-v0.2.0) (2025-11-09)

### Features

- implement basic rate limiting ([e814366](https://github.com/vacs-project/vacs/commit/e814366e4aeb96b7ea7f825f661bc2b8d03e3c64))
- **vacs-server:** reduce controller update interval to 30s ([55b8ca9](https://github.com/vacs-project/vacs/commit/55b8ca9707b8ddd51fd4312cf8c8cbc56591672c))

## 0.1.0 (2025-10-30)

### Features

- add call error signaling message ([f66fb8b](https://github.com/vacs-project/vacs/commit/f66fb8bf1e12d04098c36af3c6b82047c6eada20))
- add protocol version to websocket login ([e778af9](https://github.com/vacs-project/vacs/commit/e778af94f3c50f807713a41b4c2553a79f82e8d6))
- add SignalingClient status and logout functionality ([6842b92](https://github.com/vacs-project/vacs/commit/6842b924505987b67437294c3a2a8b5109cfeaf0))
- change auth code exchange endpoint to POST ([fe4eb29](https://github.com/vacs-project/vacs/commit/fe4eb2978aeb4297250a4af7b1da3100748b9728))
- implement automatic reconnecting in SignalingClient ([0e71616](https://github.com/vacs-project/vacs/commit/0e716163e766064e43069474f6024550d564c3b5)), closes [#3](https://github.com/vacs-project/vacs/issues/3)
- implement VATSIM OAuth using axum-login ([1d5f2ac](https://github.com/vacs-project/vacs/commit/1d5f2accd7d60267d3bfc3142bf02ed0c4dc0daf))
- **vacs-client:** add call list event ([2076e3d](https://github.com/vacs-project/vacs/commit/2076e3dabc52584026043d1101e442068d7ca6cc)), closes [#22](https://github.com/vacs-project/vacs/issues/22)
- **vacs-client:** add config setting for update release channel ([fab181f](https://github.com/vacs-project/vacs/commit/fab181f58bb5dbe32ea1513bb77ce76a8878f42b))
- **vacs-client:** implement signaling server connection ([50a2b5c](https://github.com/vacs-project/vacs/commit/50a2b5cb72bd605934abf6e2a3623421d98c6341))
- **vacs-core:** implement basic signaling client with login flow ([305ac9c](https://github.com/vacs-project/vacs/commit/305ac9cc6e2b6be56981a396fb540477963a982e))
- **vacs-core:** implement VATSIM Slurper API client for retrieving station name ([a06a735](https://github.com/vacs-project/vacs/commit/a06a735ed3405e407132617d67bda1961c4fa223))
- **vacs-server:** add auth endpoint returning logged in user's info ([96d9724](https://github.com/vacs-project/vacs/commit/96d972478221c8a1bf555c60504f6d26ea285177))
- **vacs-server:** add build information and redis health check ([b089376](https://github.com/vacs-project/vacs/commit/b0893766fae4a2ed2261878a4f21c4fdd87a04ab))
- **vacs-server:** add bundle_type parsing to update check ([c813677](https://github.com/vacs-project/vacs/commit/c813677b50e51ba91896de0280ab1143c8e5d92f))
- **vacs-server:** add client IP with configurable extractor to request span ([037f4fa](https://github.com/vacs-project/vacs/commit/037f4fae21d846e18fa33fd12c7994e80334f968)), closes [#53](https://github.com/vacs-project/vacs/issues/53)
- **vacs-server:** add configuration handling ([bf7cd6e](https://github.com/vacs-project/vacs/commit/bf7cd6e757d5f4bb0a81565c715953b955d5cf95))
- **vacs-server:** add endpoint to terminate websocket connection ([3a53321](https://github.com/vacs-project/vacs/commit/3a5332161706764e94a969a3193915fd2085a696))
- **vacs-server:** add logout auth endpoint ([790714d](https://github.com/vacs-project/vacs/commit/790714d63001204e969b0365f37f20f9e72f55fc))
- **vacs-server:** change login timeout to use millis for faster integration testing ([9c12a1a](https://github.com/vacs-project/vacs/commit/9c12a1a192e6e81113d244aa9eedb4907a33db1e))
- **vacs-server:** check for active VATSIM connection on login ([a1c7726](https://github.com/vacs-project/vacs/commit/a1c772695251a0e47e59715e2689b76a69519ca2))
- **vacs-server:** disconnect clients if no Pong reply is received in time ([9df9818](https://github.com/vacs-project/vacs/commit/9df9818a48a8037fe332ad6785d278f2a938537e))
- **vacs-server:** extend ProblemDetails to parse from StatusCode ([98f853c](https://github.com/vacs-project/vacs/commit/98f853c74b7868e6c92c8b26680febfed41ba2ae))
- **vacs-server:** filter own client from client list ([6c05d7d](https://github.com/vacs-project/vacs/commit/6c05d7d15ee587d1b2c08daf1e4331419a4b173b))
- **vacs-server:** handle new CallInvite and CallAccept signaling messages ([8769f79](https://github.com/vacs-project/vacs/commit/8769f79a6ed467c5e4748ab46ca1c5415b20f30b))
- **vacs-server:** implement basic login flow and message handling ([7c75613](https://github.com/vacs-project/vacs/commit/7c7561322439dc6cf796f09698207f8afb640336))
- **vacs-server:** implement controller update task ([d524c05](https://github.com/vacs-project/vacs/commit/d524c05cd4e1995be7cdd0c288cedb71945fd909))
- **vacs-server:** implement release update check ([d755dce](https://github.com/vacs-project/vacs/commit/d755dce652b456cbe0402d4a0405bf8c70f4440e))
- **vacs-server:** receive from websocket in separate tokio task ([fa92b7e](https://github.com/vacs-project/vacs/commit/fa92b7e00501be603cc88c5268ab0ae9bab4793a))
- **vacs-server:** return Error to client when sending message to peer fails ([8455124](https://github.com/vacs-project/vacs/commit/845512422ca555150941ae38c48f08ed288a5af5))
- **vacs-server:** send Disconnected message before terminating client ([a0017a8](https://github.com/vacs-project/vacs/commit/a0017a86aeb7f53300c17cb855880c55317d2565))
- **vacs-server:** send websocket Close frame on login failure ([0933605](https://github.com/vacs-project/vacs/commit/0933605a3593e32d1f0db0d15ca3f20f447c371e))
- **vacs-server:** skip tracing of healthcheck endpoint and favicon ([b78ef23](https://github.com/vacs-project/vacs/commit/b78ef23d2f2e7b4c542c7dfde89fd61f1d5bf60d))

### Bug Fixes

- add mock data feed to fix tests ([d6bb75b](https://github.com/vacs-project/vacs/commit/d6bb75bed19fb52a27b6f5b883c8d6b159affddd))
- continue fixing client tests ([8bf41b9](https://github.com/vacs-project/vacs/commit/8bf41b9b5c93b95f064315e4b4511b2e169ad632))
- fix tests after login refactor ([8d2c2d6](https://github.com/vacs-project/vacs/commit/8d2c2d626c75acf15dd6dc771315b3816cf209fe))
- fix tests after signaling message serialization changes ([b9eed16](https://github.com/vacs-project/vacs/commit/b9eed163250fda5764401ad829f6911b036e406c))
- **vacs-server:** disconnect client if facility changed to unknown ([aa5fc0c](https://github.com/vacs-project/vacs/commit/aa5fc0cf2d86034cef73f47b78e1092ecb037ba6))
- **vacs-server:** fix default VATSIM auth redirect url ([2a8a846](https://github.com/vacs-project/vacs/commit/2a8a84678fe0d8217285d5327fd8ac8189bcf302))
- **vacs-server:** fix login requirement for VATSIM auth routes ([951fbba](https://github.com/vacs-project/vacs/commit/951fbba4d270dd7ef4df3b29709a3f8c755dd6d0))
- **vacs-server:** fix tests after refactor ([4389afd](https://github.com/vacs-project/vacs/commit/4389afdba4fd523bbbfe240e7feddb8994f28653))
- **vacs-server:** fix trait impl for AuthnBackend ([460435e](https://github.com/vacs-project/vacs/commit/460435ea63332d7ec75e85fd65e4fdfa8da98caf))
- **vacs-server:** prevent tracing span leaking through axum handlers ([b55e3ea](https://github.com/vacs-project/vacs/commit/b55e3eae67b5568f6da872533d7626a4d51a22ab))
