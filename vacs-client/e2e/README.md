# vacs-client E2E tests

End-to-end tests driving real `vacs-client` instances via
[tauri-driver](https://tauri.app/develop/tests/webdriver/) and WebdriverIO,
against a locally spawned `vacs-server` and a mock VATSIM backend.

## Prerequisites

- `tauri-driver` (`cargo install tauri-driver --locked`)
- `WebKitWebDriver` (Linux: `webkit2gtk-driver` / `webkit2gtk4.1` package)
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

- `wdio.conf.ts`: two app instances (`clientA`/`clientB`) via two tauri-driver
  processes, covering login, calls, stations/coverage, settings and reconnect
  behavior. Specs live in `specs/`.
- `wdio.remote.conf.ts`: one app instance plus a managed headless Chromium
  acting as a remote-control browser. Specs live in `specs-remote/`.

Individual runs: `npx wdio run wdio.conf.ts --spec ./specs/call.e2e.ts`
(optionally `--mochaOpts.grep "<test name>"`).

## How it works

- The client is built with the `e2e` cargo feature: mock audio backend, no
  config file parsing, no single-instance/deep-link plugins, compile-time
  backend URL `127.0.0.1:4568`, a separate bundle identifier
  (`tauri.e2e.conf.json`) so config writes stay out of the real client's
  directories, and the `auth_login_test` command for browserless OAuth.
- `wdio.conf.ts` spawns `vatsim-mock` (port 4567, seeded from `seed.json`)
  and `vacs-server` (port 4568, env-configured against the mock, rate
  limiters disabled, 1s datafeed polling, 2s position grace period).
- `helpers/auth.ts` talks to the mock's CRUD API (`seedController`,
  `removeController`, `resetMockState`) to drive datafeed changes mid-test.
- `helpers/signaling-client.ts` is a raw WebSocket protocol client used as
  additional call participants beyond the two app instances.
- `helpers/server-control.ts` stops/starts the spawned `vacs-server` for
  outage and reconnect tests.
- Seeded users: CIDs `10000001`-`10000007`; `10000001`-`10000003` also have
  datafeed controllers (LOVV positions), `10000004`+ stay positionless with
  their CID as display name.

## Known gaps (deliberately untested here)

- Real deep-link OAuth (disabled under the `e2e` feature; manual testing)
- Global keybinds/PTT (OS-level input), window management, updater
- Volume slider dragging (custom mouse-drag widget without DOM state)
- Audio content (the mock backend produces silence by design)
- Radio/TrackAudio integration (needs a TrackAudio mock, not yet scoped)
