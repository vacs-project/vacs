# vacs-client E2E tests

End-to-end tests driving real `vacs-client` instances via WebdriverIO and
[@wdio/tauri-service](https://webdriver.io/docs/desktop-testing/tauri/) with
its embedded driver, against a locally spawned `vacs-server` and a mock
VATSIM backend. The embedded driver serves classic W3C WebDriver over HTTP
from inside the app process (`tauri-plugin-wdio-webdriver`, compiled only
with the `e2e` cargo feature), so no native platform driver is needed and
the suite runs on Linux, Windows and macOS alike.

## Prerequisites

- A Redis-compatible store on port 6379 (`docker compose up -d` in the repo root)
- The [vacs-data](https://github.com/vacs-project/vacs-data) dataset checkout
  next to this repository (or set `VACS_DATA_DIR`)
- The `vatsim-mock` binary from
  [vatsim-api](https://github.com/MorpheusXAUT/vatsim-api): either set
  `VATSIM_API_ROOT` to a checkout (it is built from source automatically) or
  install it to PATH (`cargo install --git ... --features mock-bin --bin vatsim-mock`)

## Running

```sh
cd vacs-client
npm test -w e2e
```

This runs two WebdriverIO configs in sequence:

- `wdio.conf.ts`: two app instances (`clientA`/`clientB`, embedded WebDriver
  on ports 4450/4451), covering login, calls, stations/coverage, settings and
  reconnect behavior. Specs live in `specs/`.
- `wdio.remote.conf.ts`: one app instance (port 4460) plus a managed headless
  Chromium acting as a remote-control browser. Specs live in `specs-remote/`.

Individual runs: `npx wdio run wdio.conf.ts --spec ./specs/call.e2e.ts`
(optionally `--mochaOpts.grep "<test name>"`).

## How it works

- The client is built with the `e2e` cargo feature: mock audio backend, no
  config file parsing, no single-instance/deep-link plugins, compile-time
  backend URL `127.0.0.1:4568`, a separate bundle identifier
  (`tauri.e2e.conf.json`) so config writes stay out of the real client's
  directories, the `auth_login_test` command for browserless OAuth, and the
  wdio automation plugins (`tauri-plugin-wdio` + `tauri-plugin-wdio-webdriver`,
  permitted by an inline capability in `tauri.e2e.conf.json`). None of this
  is compiled into regular builds.
- `wdio.conf.ts` spawns `vatsim-mock` (port 4567, seeded from `seed.json`)
  and `vacs-server` (port 4568, env-configured against the mock, rate
  limiters disabled, 1s datafeed polling, 2s position grace period).
  @wdio/tauri-service launches the initial app instances.
- `helpers/app-control.ts` owns app-process isolation: the embedded driver
  lives inside the app process, so a wdio `reloadSession()` alone would reuse
  the same still-logged-in process. Specs call `restartApps()` in
  `beforeEach` instead, which SIGKILLs every instance (a graceful close would
  persist the cookie store and leak the authenticated session into the next
  process), clears persisted cookies, respawns, and re-creates the sessions.
- `helpers/auth.ts` talks to the mock's CRUD API (`seedController`,
  `removeController`, `resetMockState`) to drive datafeed changes mid-test.
- `helpers/signaling-client.ts` is a raw WebSocket protocol client used as
  additional call participants beyond the two app instances.
- `helpers/server-control.ts` stops/starts the spawned `vacs-server` for
  outage and reconnect tests.
- Seeded users: CIDs `10000001`-`10000007`; `10000001`-`10000003` also have
  datafeed controllers (LOVV positions), `10000004`+ stay positionless with
  their CID as display name. Note: once a client connected with CID
  `10000001`-`10000003` leaves the 2s grace period, the server assigns it
  the corresponding position and CID-based selectors stop matching; use
  `removeController` when such CIDs must stay positionless.
- The `browser.tauri.*` API (`tauri-plugin-wdio`) is available in specs for
  IPC mocking, event emission and log capture; backend/frontend log capture
  is enabled in CI.

## Spec gotchas

- Do not hold element handles across a re-render that replaces their DOM
  node (the call queue's answer keys, the error overlay): wdio reports the
  stale handle as "not displayed" for the entire wait without refetching.
  Query fresh at each use (`const el = () => client.$(sel)`).
- `restartApps()` waits for the app document before returning; keep it that
  way, since a script executed mid-navigation loses its result and burns the
  full 30s script timeout.

## Known gaps (deliberately untested here)

- Real deep-link OAuth (disabled under the `e2e` feature; manual testing).
  The service's `triggerDeeplink` could cover this now, but needs the
  deep-link plugin re-enabled under `e2e` and a single-instance rethink.
- Global keybinds/PTT (OS-level input), window management, updater
- Volume slider dragging (custom mouse-drag widget without DOM state)
- Audio content (the mock backend produces silence by design)
- Radio/TrackAudio integration (needs a TrackAudio mock, not yet scoped)
