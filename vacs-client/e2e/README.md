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
- IPC command mocks go through `helpers/browser.ts` (`mockCommand`,
  `unmockCommand`), which write the wdio mock registry directly into the
  page; the transport consults it in e2e builds (`withGlobalTauri` is
  enabled in `tauri.e2e.conf.json` for the plugin's globals). Do not use
  `browser.tauri.mock`: its worker-side store reuses mock objects across
  sessions and breaks after `restartApps()`. `browser.tauri.emitEvent` is
  equally off-limits with the embedded provider (its eval wrapper exposes
  only the core API); emit through `tauriApi(...).execute` and
  `window.__TAURI__.event.emit` instead. `browser.tauri.execute` and log
  capture work as documented; backend/frontend log capture is enabled in CI.

## Spec gotchas

- Do not hold element handles across a re-render that replaces their DOM
  node (the call queue's answer keys, the error overlay): wdio reports the
  stale handle as "not displayed" for the entire wait without refetching.
  Query fresh at each use (`const el = () => client.$(sel)`).
- `restartApps()` waits for the app document before returning; keep it that
  way, since a script executed mid-navigation loses its result and burns the
  full 30s script timeout.

## Documentation screenshots

`npm run -w e2e screenshots` runs `wdio.docs.ts` over `specs-docs/`, which
captures the images the user manual needs into `e2e/screenshots/` (override
with `VACS_SCREENSHOT_DIR`, e.g. the docs repo's `static/img`). It reuses the
regular config's servers and app instances and is kept out of `npm test`,
since these specs produce artifacts rather than assert behavior.

Everything that would otherwise differ between runs is pinned, so a single
re-captured image still matches the rest of the set: both clients log in with
fixed CIDs and positions (`10000001`/`LOVV_E_CTR`), the webview's clock is
frozen at 10:10:10Z, the version in the header is set explicitly, and the
platform capabilities the UI renders from are mocked to `LinuxX11`. That last
one matters more than it looks: the keybind pages have a separate Wayland
layout, so capturing on a Wayland desktop would otherwise put the System
Shortcuts button and desktop-managed key fields into images meant to show the
ordinary one. The version defaults to `vacs-client/package.json`; export
`VACS_SCREENSHOT_VERSION=2.6.0` when capturing for a release that has not been
cut yet.

Image names follow the manual's convention: `XConfig.png` is the settings page
with callouts showing how to open X, `XConfigPage.png` is the dialog itself,
and Transmit dialog crops are named after the combination they show
(`Transmit-<mic mode>-<integration>.png`), with `-wayland` appended for the
Wayland variant.

Two properties of the setup shape what the images look like:

- The embedded driver snapshots the **webview only**, so captures carry no
  title bar or window border. Existing manual images that were taken with a
  window manager screenshot are 32px taller for that reason.
- Element screenshots return the full frame with the element scrolled into
  view, so `helpers/screenshot.ts` crops the PNG itself from the element's
  bounding rect.

Callouts come from `helpers/annotate.ts`, in the manual's established style:
a red box around the element and a numbered badge on one of its corners, with
the numbers explained in the prose beside the image. Both are drawn as an SVG
overlay positioned from the target's bounding rect, so re-capturing keeps them
on the right element instead of leaving them where the layout used to be.
`place` picks the corner or side the badge sits on, which is how you keep it clear
of neighboring UI. Call `clearAnnotations()` before capturing anything else in
the same test.

States that need hardware, another platform or a broken network are driven
through IPC mocks and emitted events (joystick devices, the Wayland layout,
a degraded call, the radio error state). That is honest for a layout
screenshot and useless as verification: an image says the UI renders that
state, never that the underlying platform behavior works.

## Known gaps (deliberately untested here)

- Real deep-link OAuth (disabled under the `e2e` feature; manual testing).
  The service's `triggerDeeplink` could cover this now, but needs the
  deep-link plugin re-enabled under `e2e` and a single-instance rethink.
- Global keybinds/PTT (OS-level input), window management, updater
- Volume slider dragging (custom mouse-drag widget without DOM state)
- Audio content (the mock backend produces silence by design)
- Radio/TrackAudio integration (needs a TrackAudio mock, not yet scoped)
