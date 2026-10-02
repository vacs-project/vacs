import {readFileSync} from "node:fs";
import path from "node:path";
import {fileURLToPath} from "node:url";
import type {PNG} from "pngjs";
import type {ChainablePromiseElement} from "webdriverio";
import {loginAndConnectAs, removeController, seedController} from "./auth.ts";
import {click, getClient, mockCommand, pageButton, tauriApi} from "./browser.ts";
import {writeGif} from "./gif.ts";
import {SignalingTestClient} from "./signaling-client.ts";
import {captureFrame, freezeClock, type Rect, writeImage} from "./screenshot.ts";

/**
 * Fixtures and page helpers shared by the documentation screenshot specs in
 * specs-docs/. Everything that would otherwise differ between runs is pinned
 * here, so a single re-captured image still matches the rest of the set.
 */

const __dirname = fileURLToPath(new URL(".", import.meta.url));

/** The webview clock. UTC, which is what the header shows. */
export const CLOCK = "2026-01-01T10:10:10Z";
export const CLOCK_SECS = Date.parse(CLOCK) / 1000;

/**
 * The version in the header. Defaults to the client's current version; set
 * VACS_SCREENSHOT_VERSION when capturing for a release that is not cut yet.
 * Empty counts as unset: an omitted workflow_dispatch input arrives as "".
 */
export const VERSION =
    process.env.VACS_SCREENSHOT_VERSION ||
    (
        JSON.parse(readFileSync(path.resolve(__dirname, "..", "..", "package.json"), "utf8")) as {
            version: string;
        }
    ).version;

/** The update the update images advertise: the next minor release. */
export const NEXT_VERSION = (() => {
    const [major, minor] = VERSION.split(".").map(Number);
    return `${major}.${minor + 1}.0`;
})();

// Tabbed layout images move CID_A to LOWW_APP through seedPosition rather than
// using another CID, so the header shows the same CID throughout the manual.
export const CID_A = "10000001";
export const POSITION_A = "LOVV_E_CTR";
export const TABBED_POSITION = "LOWW_APP";
export const TABBED_FREQUENCY = "134.675";

/**
 * Users without a datafeed controller, which keep whatever position they log
 * in with: the other parties of the captured calls, as app instances or raw
 * signaling clients.
 */
export const CID_B = "10000005";
export const CID_C = "10000006";
export const CID_D = "10000007";
/**
 * A positionless user for the profile images: a resolved position would have
 * the server push its own profile over a profile loaded from a file.
 */
export const CID_PROFILE = "10000004";

/** The datafeed's BC controller, which masks the S stations while it is not on vacs. */
export const DATAFEED_BC_CID = "10000003";

// The keybind pages render differently per platform, and the capture host's
// own session decides which variant you get: run this on a Wayland desktop
// and every image picks up the System Shortcuts button and desktop-managed
// key fields. Pin the platform so an image does not depend on where it was
// taken. X11 renders what Windows and macOS render, which is what the rest
// of the manual's images show.
export const DESKTOP_CAPABILITIES = {
    alwaysOnTop: true,
    keybindListener: true,
    keybindEmitter: true,
    joystick: true,
    playback: true,
    platform: "LinuxX11",
};

// A TrackAudio integration without a TrackAudio instance behind it. Mocked
// rather than set through the settings page: the real integration starts a
// connection whose retries would overwrite the radio state mid-capture.
export const TRACK_AUDIO_CONFIG = {
    integration: "TrackAudio",
    audioForVatsim: null,
    trackAudio: {endpoint: "127.0.0.1:49080"},
};

/** Metadata of one frequency object. The mock audio backend carries no radio traffic. */
export type RadioStationFixture = {
    callsign: string;
    frequency: number;
    rx: boolean;
    tx: boolean;
    xc: boolean;
    xca: boolean;
    headset: boolean;
    output_muted: boolean;
    is_available: boolean;
};

export function radioStation(
    callsign: string,
    frequency: number,
    state: Partial<Omit<RadioStationFixture, "callsign" | "frequency">> = {},
): RadioStationFixture {
    return {
        callsign,
        frequency,
        rx: false,
        tx: false,
        xc: false,
        xca: false,
        headset: true,
        output_muted: false,
        is_available: true,
        ...state,
    };
}

// Recordings behind the Playback page images. The times sit shortly before
// the frozen clock; callsigns and frequency follow the manual's earlier
// playback image.
export const CLIPS = [
    {secsAgo: 46, durationMs: 3500, callsigns: ["AUA2PJ"]},
    {secsAgo: 56, durationMs: 2200, callsigns: ["AUA2PJ"]},
    {secsAgo: 88, durationMs: 7200, callsigns: ["AUA25"]},
    {secsAgo: 92, durationMs: 2900, callsigns: ["EWG1GM"]},
    {secsAgo: 104, durationMs: 3900, callsigns: ["EWG1GM"]},
    {secsAgo: 112, durationMs: 5900, callsigns: ["AUA99"]},
    {secsAgo: 139, durationMs: 5600, callsigns: ["SWR8SW"]},
].map((clip, index) => {
    const endedSecs = CLOCK_SECS - clip.secsAgo;
    const startedSecs = endedSecs - clip.durationMs / 1000;
    return {
        id: index + 1,
        path: `/playback/${index + 1}.wav`,
        callsigns: clip.callsigns,
        frequency: 122_125_000,
        startedAt: {
            secs_since_epoch: Math.floor(startedSecs),
            nanos_since_epoch: Math.round((startedSecs % 1) * 1e9),
        },
        endedAt: {secs_since_epoch: endedSecs, nanos_since_epoch: 0},
        durationMs: clip.durationMs,
    };
});

/** The wrench in the window header that opens the settings page. */
export const SETTINGS_BUTTON = '//button[.//img[@alt="Settings"]]';
/** The button in the right hand column that opens the telephone page. */
export const TELEPHONE_BUTTON = '//button[.//img[@alt="Telephone"]]';
/** The button in the right hand column that opens the mission page. */
export const MISSION_BUTTON = '//button[.//img[@alt="Mission"]]';
/** The call status indicator in the window's top left corner. */
export const STATUS_INDICATOR =
    '//div[contains(@title, "Click to switch to")]//div[contains(@class, "rounded-full")]';
/** The clock, which also hosts the status indicator. */
export const CLOCK_CELL = '//div[contains(@title, "Click to switch to")]';
/** The call display: the topmost call queue slot, which shows the own call. */
export const CALL_DISPLAY =
    '//div[contains(@class, "scrollbar-none")]' +
    '/div[contains(@class, "relative")]/button[contains(@class, "h-16")]';
/**
 * An incoming call's answer key: a call queue slot that is a direct child of
 * the queue, unlike the call display, which sits in a wrapper. Matched
 * structurally because a conference invitation is labeled CONF rather than
 * with the caller's name.
 */
export const INCOMING_ANSWER_KEY =
    '//div[contains(@class, "scrollbar-none")]/button[contains(@class, "h-16")]';
/** The ordinary Phone page button of the bottom row. */
export const PHONE_BUTTON = '//button[.//p[not(@title) and text()="Phone"]]';

/** A direct access key carrying the given label lines, e.g. ["ACC", "N1", "EC"]. */
export function stationKey(label: string[]): string {
    return `//button[${label.map(line => `.//p[@title="${line}"]`).join(" and ")}]`;
}

/**
 * Pins everything in the window that would otherwise differ per run: the
 * clock, the version in the header, and the platform the UI renders for.
 * `newVersion` adds the header's update notice.
 */
export async function applyFixtures(
    browser: WebdriverIO.Browser,
    instanceName: string,
    options: {newVersion?: string} = {},
): Promise<void> {
    await mockCommand(instanceName, "app_platform_capabilities", {resolve: DESKTOP_CAPABILITIES});
    await refetchCapabilities(instanceName);
    await freezeClock(browser, CLOCK);
    await setVersion(instanceName, VERSION, options.newVersion);

    // The clock repaints on its own timer, so the frozen time lands a tick
    // after the override.
    const clock = browser.$(CLOCK_CELL);
    await browser.waitUntil(async () => (await clock.getText()).includes("10:10"), {
        timeoutMsg: "Clock did not settle on the frozen time",
    });
}

/** Sets the header's version and, optionally, the update it advertises. */
export async function setVersion(
    instanceName: string,
    version: string,
    newVersion?: string,
): Promise<void> {
    await tauriApi(instanceName).execute(
        (_tauri, current: string, next: string | null) => {
            type Hooks = {setVersion: (version: string, newVersion?: string) => void};
            const w = window as Window & {__vacs_e2e__?: Hooks};
            if (w.__vacs_e2e__ === undefined) throw new Error("E2E hooks are not installed");
            w.__vacs_e2e__.setVersion(current, next ?? undefined);
        },
        version,
        newVersion ?? null,
    );
}

/**
 * Moves a seeded user's datafeed controller to another position. A CID with a
 * datafeed controller gets that controller's position from the server once the
 * grace period runs out, whatever it logged in with, so the position the image
 * needs has to be in the datafeed. Call before the client connects.
 */
export async function seedPosition(
    cid: string,
    callsign: string,
    frequency: string,
    facility: number,
): Promise<void> {
    await removeController(cid);
    await seedController({
        cid: Number(cid),
        name: `Mock Controller ${cid.slice(-1)}`,
        callsign,
        frequency,
        facility,
        rating: 0,
        server: "MOCK",
        visual_range: 50,
        text_atis: [],
        last_updated: "1970-01-01T00:00:00.000000Z",
        logon_time: "1970-01-01T00:00:00.000000Z",
    });
}

/** Puts CID_A on the tabbed LOWW_APP position in the datafeed. */
export async function seedTabbedPosition(): Promise<void> {
    await seedPosition(CID_A, TABBED_POSITION, TABBED_FREQUENCY, 5);
}

/**
 * Selects the TrackAudio integration and pins the radio to a state, without a
 * TrackAudio instance and without touching the real configuration.
 */
export async function applyTrackAudioMocks(
    instanceName: string,
    state: "Connected" | "Disconnected" | "RxIdle",
    stations: unknown[] = [],
    options: {cplMode?: "Original" | "Fast"} = {},
): Promise<void> {
    await mockCommand(instanceName, "radio_get_config", {resolve: TRACK_AUDIO_CONFIG});
    await mockCommand(instanceName, "radio_get_stations", {resolve: stations});
    await mockCommand(instanceName, "app_get_cpl_mode", {resolve: options.cplMode ?? "Original"});
    // Opening the radio page and the Retry link both attempt a reconnect,
    // which the backend has no integration to serve.
    await mockCommand(instanceName, "radio_reconnect", {resolve: null});
    await refetchSettings(instanceName);
    await emitEvent(instanceName, "radio:state", {state});
}

/**
 * Puts the radio playback UI into its working state without a TrackAudio
 * instance: a configured integration, recording enabled, a connected radio,
 * and recordings to list. The mock audio backend records nothing, so the
 * clips are metadata only.
 */
export async function applyRadioPlaybackMocks(instanceName: string): Promise<void> {
    await mockCommand(instanceName, "playback_get_enabled", {resolve: true});
    await mockCommand(instanceName, "playback_list", {resolve: CLIPS});
    await applyTrackAudioMocks(instanceName, "Connected");
}

/**
 * Loads a profile from a file, the only way to get a layout the dataset the
 * harness serves does not publish.
 */
export async function loadTestProfile(browser: WebdriverIO.Browser, file: string): Promise<void> {
    const result = await browser.execute(async (profilePath: string) => {
        try {
            await window.__TAURI_INTERNALS__.invoke("app_load_test_profile", {path: profilePath});
            return {ok: true as const};
        } catch (e) {
            return {ok: false as const, error: String(e)};
        }
    }, file);

    if (!result.ok) throw new Error(`app_load_test_profile failed for ${file}: ${result.error}`);
}

/** Re-runs the capability fetch, which otherwise only happens on mount. */
export async function refetchCapabilities(instanceName: string): Promise<void> {
    await tauriApi(instanceName).execute(() => {
        type Hooks = {refetchCapabilities: () => Promise<void>};
        const w = window as Window & {__vacs_e2e__?: Hooks};
        if (w.__vacs_e2e__ === undefined) throw new Error("E2E hooks are not installed");
        void w.__vacs_e2e__.refetchCapabilities();
    });
}

/** Re-runs the settings fetch, so mocked settings commands take effect. */
export async function refetchSettings(instanceName: string): Promise<void> {
    await tauriApi(instanceName).execute(() => {
        type Hooks = {refetchSettings: () => Promise<void>};
        const w = window as Window & {__vacs_e2e__?: Hooks};
        if (w.__vacs_e2e__ === undefined) throw new Error("E2E hooks are not installed");
        void w.__vacs_e2e__.refetchSettings();
    });
}

/** The id of the call on the call display, or null. */
export async function activeCallId(instanceName: string): Promise<string | null> {
    return (await tauriApi(instanceName).execute(() => {
        type Hooks = {activeCallId: () => string | null};
        const w = window as Window & {__vacs_e2e__?: Hooks};
        if (w.__vacs_e2e__ === undefined) throw new Error("E2E hooks are not installed");
        return w.__vacs_e2e__.activeCallId();
    })) as string | null;
}

/** Emits an event into the page as if the backend had sent it. */
export async function emitEvent(
    instanceName: string,
    event: string,
    payload: unknown,
): Promise<void> {
    await tauriApi(instanceName).execute(
        (_tauri, name: string, data: unknown) => {
            type TauriGlobal = {event: {emit: (name: string, payload?: unknown) => Promise<void>}};
            const w = window as Window & {__TAURI__?: TauriGlobal};
            if (w.__TAURI__ === undefined) throw new Error("Tauri globals are not available");
            void w.__TAURI__.event.emit(name, data);
        },
        event,
        payload,
    );
}

/** Waits until a direct access key is clickable, i.e. its station is online. */
export async function waitUntilEnabled(
    browser: WebdriverIO.Browser,
    element: ChainablePromiseElement,
    label: string,
): Promise<void> {
    await element.waitForDisplayed();
    await browser.waitUntil(async () => await element.isEnabled(), {
        timeoutMsg: `${label} did not come online`,
    });
}

/**
 * Waits until the call status indicator is green, which is the point at
 * which every peer of the current call carries media. Capturing before it
 * would show a call still negotiating.
 */
export async function waitForConnectedCall(browser: WebdriverIO.Browser): Promise<void> {
    const indicator = browser.$(STATUS_INDICATOR);
    await browser.waitUntil(
        async () => ((await indicator.getAttribute("class")) ?? "").includes("bg-green"),
        {timeoutMsg: "Call did not reach the connected state"},
    );
}

/**
 * Whether the Phone button is in the colored half of its blink cycle. Every
 * blinking call state (incoming, outgoing priority, rejected, errored) blinks
 * the Phone button in step with the keys, and it is gray in the other half.
 */
export function phoneButtonLit(browser: WebdriverIO.Browser): () => Promise<boolean> {
    return async () =>
        !((await browser.$(PHONE_BUTTON).getAttribute("class")) ?? "").includes("bg-gray-300");
}

/**
 * Captures the window while a blinking state is in the given half of its
 * cycle. The blink toggles every 500ms, so the capture waits for a phase
 * change, lets the color settle, and checks the phase again afterwards rather
 * than trusting the usual settle pause, which would land in whichever half
 * came next.
 */
export async function capturePhase(
    browser: WebdriverIO.Browser,
    isLit: () => Promise<boolean>,
    lit: boolean,
    rect?: Rect,
): Promise<PNG> {
    for (let attempt = 0; attempt < 10; attempt++) {
        await browser.waitUntil(async () => (await isLit()) !== lit, {
            interval: 20,
            timeoutMsg: "The blink did not change phase",
        });
        await browser.waitUntil(async () => (await isLit()) === lit, {
            interval: 20,
            timeoutMsg: "The blink did not change phase back",
        });
        const png = await captureFrame(browser, {rect, settle: 160});
        if ((await isLit()) === lit) return png;
    }
    throw new Error("Could not capture inside one half of the blink cycle");
}

/** The blink period's half, in milliseconds (see blink-store.ts). */
const BLINK_HALF_MS = 500;

/**
 * Records a blinking state as a two-frame looping GIF: one frame per half of
 * the blink cycle, each shown for as long as the UI shows it. Equivalent to a
 * screen recording of the state, without the recording's dropped and doubled
 * frames.
 */
export async function captureBlinkGif(
    browser: WebdriverIO.Browser,
    name: string,
    options: {isLit?: () => Promise<boolean>; rect?: Rect} = {},
): Promise<string> {
    const isLit = options.isLit ?? phoneButtonLit(browser);
    const lit = await capturePhase(browser, isLit, true, options.rect);
    const dark = await capturePhase(browser, isLit, false, options.rect);
    return writeGif(name, [
        {png: lit, delay: BLINK_HALF_MS},
        {png: dark, delay: BLINK_HALF_MS},
    ]);
}

/** Captures the window, or a region of it, in the colored half of a blink cycle. */
export async function captureLit(
    browser: WebdriverIO.Browser,
    name: string,
    options: {isLit?: () => Promise<boolean>; rect?: Rect} = {},
): Promise<string> {
    const isLit = options.isLit ?? phoneButtonLit(browser);
    return writeImage(await capturePhase(browser, isLit, true, options.rect), name);
}

export async function openSettings(browser: WebdriverIO.Browser): Promise<void> {
    const settingsButton = await browser.$(SETTINGS_BUTTON);
    await settingsButton.waitForDisplayed();
    await click(browser, settingsButton);
}

/**
 * A settings page button. Exact text match: a substring match on "Call" would
 * also hit the call controls on the page behind the settings menu.
 */
export function settingsPageButtonSelector(label: string): string {
    return `//button[./p[text()="${label}"]]`;
}

export async function openSettingsPage(browser: WebdriverIO.Browser, label: string): Promise<void> {
    const button = await browser.$(settingsPageButtonSelector(label));
    await button.waitForDisplayed();
    await click(browser, button);
}

/** A settings sub page dialog carrying the given title. */
export function subPage(browser: WebdriverIO.Browser, title: string): ChainablePromiseElement {
    return browser.$(subPageSelector(title));
}

export function subPageSelector(title: string): string {
    return `//div[./p[text()="${title}"]]`;
}

/** Opens the Radio page, retrying until the mocked radio config has reached the store. */
export async function openRadioPage(
    browser: WebdriverIO.Browser,
    ready: ChainablePromiseElement,
): Promise<void> {
    await browser.waitUntil(
        async () => {
            if (await ready.isDisplayed()) return true;
            await click(browser, pageButton(browser, "Radio"));
            return await ready.isDisplayed();
        },
        {timeoutMsg: "The Radio page did not open"},
    );
}

/** Where a call claims to come from, which is what the callee's keys highlight. */
export type CallOrigin = {cid: string; positionId: string; stationId?: string};

/**
 * Starts a call from an app instance through the command its direct access
 * keys invoke, with an explicit call source. Pressing the caller's own key
 * first would do the same through two more page navigations on a client
 * nobody looks at.
 */
export async function inviteFrom(
    browser: WebdriverIO.Browser,
    origin: CallOrigin,
    target: {client?: string; station?: string},
    options: {prio?: boolean} = {},
): Promise<void> {
    const result = await browser.execute(
        async (source: CallOrigin, callTarget: {client?: string}, prio: boolean) => {
            try {
                await window.__TAURI_INTERNALS__.invoke("signaling_invite_to_call", {
                    source: {
                        clientId: source.cid,
                        positionId: source.positionId,
                        stationId: source.stationId,
                    },
                    targets: [callTarget],
                    prio,
                });
                return {ok: true as const};
            } catch (e) {
                return {ok: false as const, error: String(e)};
            }
        },
        origin,
        target,
        options.prio ?? false,
    );
    if (!result.ok) throw new Error(`signaling_invite_to_call failed: ${result.error}`);
}

/** Accepts the first incoming call on an app instance, as its answer key would. */
export async function acceptIncoming(browser: WebdriverIO.Browser): Promise<void> {
    const answerKey = browser.$(INCOMING_ANSWER_KEY);
    await answerKey.waitForDisplayed();
    await click(browser, answerKey);
}

/** The tab button of a tabbed profile carrying the given label. */
export function tabButton(label: string): string {
    return `//button[contains(@class, "rounded-b-lg")][.//p[@title="${label}"]]`;
}

// The keys of the LOWW tabbed profile's EC tab the call images use.
export const KEY_N1 = stationKey(["ACC", "N1", "EC"]);
export const KEY_E1 = stationKey(["ACC", "E1", "EC"]);
export const KEY_VB = stationKey(["APP", "VB-EC", "134675"]);
export const KEY_VN = stationKey(["APP", "VN-EC", "118775"]);
export const KEY_VD1 = stationKey(["APP", "VD1"]);
export const KEY_TFI = stationKey(["APP", "TFI", "EC"]);
/** A station of another FIR that nobody staffs in the harness: a key that is offline. */
export const KEY_PRA_LW = stationKey(["PRA", "LW", "EC"]);
/**
 * The empty key below MCC TAU, which references no station. Keys are laid out
 * column by column, so it is the next key in document order.
 */
export const KEY_WITHOUT_STATION = `${stationKey(["MCC", "TAU", "EC"])}/following-sibling::button[1]`;

/** A stand-in for the A neighbor of the tabbed images, see setupTabbed. */
export type TabbedNeighbor = "app" | "rejecting" | "silent";

/** Raw signaling clients a test opened, disconnected by closeRawClients(). */
const rawClients: SignalingTestClient[] = [];

/** Connects a raw signaling client on a position, closed again by closeRawClients(). */
export async function connectRaw(cid: string, positionId: string): Promise<SignalingTestClient> {
    const client = await SignalingTestClient.connect(cid, {positionId});
    rawClients.push(client);
    return client;
}

export function closeRawClients(): void {
    for (const client of rawClients.splice(0)) client.disconnect();
}

/** The A neighbor's call origin when it calls from one of its stations. */
export function eastOrigin(stationId: string): CallOrigin {
    return {cid: CID_B, positionId: POSITION_A, stationId};
}

/**
 * Staffs the neighborhood of the tabbed LOWW_APP images and connects clientA
 * there. LOVV_E_CTR covers the ACC keys and both APP neighbors, LOWW_D_APP
 * the VD keys and LOWW_I_APP the TFI key, which is the set the manual's
 * images show online.
 *
 * The E neighbor is clientB by default, because an answered call needs a real
 * peer for its media to connect; "rejecting" and "silent" put a raw client
 * there instead, which rejects or ignores every invitation.
 */
export async function setupTabbed(
    options: {neighbor?: TabbedNeighbor} = {},
): Promise<SignalingTestClient | undefined> {
    const clientA = getClient("clientA");
    // Covers the S stations ahead of LOVV_E_CTR while not being on vacs.
    await removeController(DATAFEED_BC_CID);
    await seedTabbedPosition();
    // The server reads the datafeed once a second and nothing on the client
    // shows when it has; a login before that would match CID_A to its old
    // position and lose LOWW_APP after the grace period.
    await clientA.pause(1500);

    await connectRaw(CID_C, "LOWW_D_APP");
    await connectRaw(CID_D, "LOWW_I_APP");

    let east: SignalingTestClient | undefined;
    const neighbor = options.neighbor ?? "app";
    if (neighbor === "app") {
        await loginAndConnectAs(getClient("clientB"), CID_B, POSITION_A);
    } else {
        east = await connectRaw(CID_B, POSITION_A);
        if (neighbor === "rejecting") east.autoRejectInvitations();
    }

    await loginAndConnectAs(clientA, CID_A, TABBED_POSITION);
    await applyFixtures(clientA, "clientA");
    await waitUntilEnabled(clientA, clientA.$(KEY_E1), "ACC E1");
    await waitUntilEnabled(clientA, clientA.$(KEY_VD1), "APP VD1");
    await waitUntilEnabled(clientA, clientA.$(KEY_TFI), "APP TFI");
    return east;
}
