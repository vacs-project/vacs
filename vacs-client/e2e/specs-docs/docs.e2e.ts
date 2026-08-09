import {readFileSync} from "node:fs";
import path from "node:path";
import {fileURLToPath} from "node:url";
import type {ChainablePromiseElement} from "webdriverio";
import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnectAs, removeController, resetMockState} from "../helpers/auth.ts";
import {
    callQueueSlot,
    click,
    getClient,
    mockCommand,
    selectOption,
    tauriApi,
    waitForCallColor,
} from "../helpers/browser.ts";
import {captureElement, captureWindow, freezeClock} from "../helpers/screenshot.ts";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

// Fixed fixtures, so re-capturing a single image does not make it differ from
// the rest of the set. The clock is UTC, which is what the header shows.
const CLOCK = "2026-01-01T10:10:10Z";
// The version in the header. Defaults to the client's current version; set
// VACS_SCREENSHOT_VERSION when capturing for a release that is not cut yet.
const VERSION =
    process.env.VACS_SCREENSHOT_VERSION ??
    (
        JSON.parse(readFileSync(path.resolve(__dirname, "..", "..", "package.json"), "utf8")) as {
            version: string;
        }
    ).version;

// Both clients take a real position, so the captured window shows a populated
// station page and a callsign in the header rather than a bare CID. This is
// what the manual's existing screenshots look like.
const CID_A = "10000001";
const POSITION_A = "LOVV_E_CTR";
// A user without a datafeed controller keeps the position it is given.
const CID_B = "10000005";
const POSITION_B = "LOVV_BC_CTR";
// The datafeed's own BC controller would mask the S stations that make the
// call in the degraded-call capture routable.
const DATAFEED_BC_CID = "10000003";

// Device metadata behind the joystick screenshots. SDL GUIDs, a throttle and
// a yoke, chosen so the images show two distinguishable products rather than
// whatever happens to be plugged into the machine taking them.
const THROTTLE = {
    device: "0300f39c4d0f00000200000000000000",
    name: "VPC Throttle",
};
const YOKE = {
    device: "030079b82341000000c0000000000000",
    name: "Alpha Yoke",
};
const THROTTLE_BUTTON = {device: THROTTLE.device, button: 3, name: THROTTLE.name};

const WAYLAND_CAPABILITIES = {
    alwaysOnTop: false,
    keybindListener: true,
    keybindEmitter: false,
    joystick: true,
    playback: true,
    platform: "LinuxWayland",
};

// Keys the desktop environment would report for the portal shortcuts. Distinct
// per action so the Wayland images do not show identical fields.
const EXTERNAL_BINDINGS = {
    AcceptCall: "Ctrl+Alt+A",
    EndCall: "Ctrl+Alt+E",
    ToggleRadioPrio: "Ctrl+Alt+R",
    PushToTalk: "Ctrl+Alt+T",
    PushToMute: "Ctrl+Alt+M",
    RadioPushToTalk: "Ctrl+Alt+P",
};

describe("Documentation screenshots", () => {
    beforeEach(async () => {
        await resetMockState();
        await restartApps();
    });

    it("captures the Call Config page", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        await openSettings(clientA);
        await openSettingsPage(clientA, "Call");
        await subPage(clientA, "Call Config").waitForDisplayed();

        await captureWindow(clientA, "settings/CallConfig.png");
    });

    it("captures the Hotkeys Config with a joystick button bound", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        // The backend captures joystick input, so the button press that would
        // normally resolve this call is what the mock stands in for. Binding
        // it is mocked as well: the device does not exist on this machine.
        await mockCommand("clientA", "keybinds_capture_joystick_button", {
            resolve: THROTTLE_BUTTON,
        });
        await mockCommand("clientA", "keybinds_set_binding", {resolve: null});

        await openSettings(clientA);
        await openSettingsPage(clientA, "Hotkeys");
        await subPage(clientA, "Hotkeys Config").waitForDisplayed();

        await click(clientA, keyField(clientA, "Accept first call"));
        await clientA.waitUntil(
            async () =>
                (await keyFieldLabel(clientA, "Accept first call").getText()) ===
                "Button 3 (VPC Throttle)",
            {timeoutMsg: "Joystick button was not shown on the binding field"},
        );

        await captureElement(
            clientA,
            subPage(clientA, "Hotkeys Config"),
            "settings/HotkeysConfigPage-joystick.png",
        );
    });

    it("captures the Joystick Devices dialog", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        await mockCommand("clientA", "keybinds_list_joystick_devices", {
            resolve: [
                {...THROTTLE, ignored: true},
                {...YOKE, ignored: false},
            ],
        });

        await openSettings(clientA);
        await openSettingsPage(clientA, "Hotkeys");
        await click(clientA, clientA.$('//button[contains(., "Joystick")]'));

        const dialog = subPage(clientA, "Joystick Devices");
        await dialog.waitForDisplayed();
        await clientA.$(`//label[text()="${YOKE.name}"]`).waitForDisplayed();

        await captureElement(clientA, dialog, "settings/JoystickDevices.png");
    });

    it("captures the Wayland variant of the Hotkeys Config", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");
        await applyWaylandMocks("clientA");

        await openSettings(clientA);
        await openSettingsPage(clientA, "Hotkeys");
        const hotkeys = subPage(clientA, "Hotkeys Config");
        await hotkeys.waitForDisplayed();
        await clientA.$('//button[contains(., "System")]').waitForDisplayed();
        await clientA.waitUntil(
            async () =>
                (await keyFieldLabel(clientA, "Accept first call").getText()) ===
                EXTERNAL_BINDINGS.AcceptCall,
            {timeoutMsg: "Desktop-managed key was not shown on the binding field"},
        );

        await captureElement(clientA, hotkeys, "settings/HotkeysConfigPage-wayland.png");
    });

    it("captures the Wayland variant of the Transmit Config", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");
        await applyWaylandMocks("clientA");

        await openSettings(clientA);
        await openSettingsPage(clientA, "Transmit");
        const transmit = subPage(clientA, "Transmit Config");
        await transmit.waitForDisplayed();
        await clientA.$('//button[contains(., "System")]').waitForDisplayed();

        // Voice activation, the default, leaves both key fields without an
        // action to map to; Push-to-talk plus TrackAudio is the combination
        // the page's Wayland note is about.
        await selectOption(clientA, 'select[name="keybind-mode"]', "PushToTalk");
        await selectOption(clientA, 'select[name="radio-integration"]', "TrackAudio");
        await clientA.waitUntil(
            async () =>
                (await clientA.$('//select[@name="keybind-mode"]').getValue()) === "PushToTalk",
            {timeoutMsg: "Call mic mode did not switch to Push-to-talk"},
        );

        await captureElement(clientA, transmit, "settings/TransmitConfig-wayland.png");
    });

    it("captures the radio button in its error state", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        const radioButton = clientA.$('//button[./p[text()="Radio"]]');
        await radioButton.waitForDisplayed();

        await emitEvent("clientA", "radio:state", {state: "Error"});
        // The red is a border and background color change, not a class the
        // DOM exposes by name, so wait on the state the button renders from.
        await clientA.pause(500);

        await captureElement(clientA, radioButton, "radio/radio_button_error.png", {padding: 8});
    });

    it("captures a degraded call", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        await removeController(DATAFEED_BC_CID);
        await restartApps();
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");
        await loginAndConnectAs(clientB, CID_B, POSITION_B);

        // S1 is covered by the other client's position, so calling the station
        // reaches it. The incoming call is labeled with our own call source.
        const group = await clientA.$('//button[.//p[@title="S"] and .//p[@title="LOWG"]]');
        await group.waitForDisplayed();
        await click(clientA, group);

        const s1 = clientA.$('//button[.//p[@title="S1"]]');
        await s1.waitForDisplayed();
        await clientA.waitUntil(async () => await s1.isEnabled(), {
            timeoutMsg: "S1 did not come online",
        });
        await click(clientA, s1);

        const answerKey = callQueueSlot(clientB, "E1");
        await answerKey.waitForDisplayed();
        await click(clientB, answerKey);
        await waitForCallColor(clientA, s1, {active: true});

        // A real one-way-audio situation needs a broken network path between
        // two hosts; the event the media watchdog would emit for it does not.
        const callId = await activeCallId("clientA");
        if (callId === null) throw new Error("No active call to degrade");
        await emitEvent("clientA", "webrtc:call-degraded", callId);

        await clientA.$('img[alt="No incoming audio"]').waitForDisplayed();

        await captureWindow(clientA, "troubleshooting/degraded-call.png");
    });
});

/**
 * Pins everything in the window that would otherwise differ per run: the
 * clock, and the version in the header.
 */
async function applyFixtures(browser: WebdriverIO.Browser, instanceName: string): Promise<void> {
    await freezeClock(browser, CLOCK);
    await tauriApi(instanceName).execute((_tauri, version: string) => {
        type Hooks = {setVersion: (version: string) => void};
        const w = window as Window & {__vacs_e2e__?: Hooks};
        if (w.__vacs_e2e__ === undefined) throw new Error("E2E hooks are not installed");
        w.__vacs_e2e__.setVersion(version);
    }, VERSION);

    // The clock repaints on its own timer, so the frozen time lands a tick
    // after the override.
    const clock = browser.$('//div[contains(@title, "Click to switch to")]');
    await browser.waitUntil(async () => (await clock.getText()).includes("10:10"), {
        timeoutMsg: "Clock did not settle on the frozen time",
    });
}

async function openSettings(browser: WebdriverIO.Browser): Promise<void> {
    const settingsButton = await browser.$('//button[.//img[@alt="Settings"]]');
    await settingsButton.waitForDisplayed();
    await click(browser, settingsButton);
}

async function openSettingsPage(browser: WebdriverIO.Browser, label: string): Promise<void> {
    // Exact text match: a substring match on "Call" would also hit the call
    // controls on the page behind the settings menu.
    const button = await browser.$(`//button[./p[text()="${label}"]]`);
    await button.waitForDisplayed();
    await click(browser, button);
}

/** The settings sub page dialog carrying the given title. */
function subPage(browser: WebdriverIO.Browser, title: string): ChainablePromiseElement {
    return browser.$(`//div[./p[text()="${title}"]]`);
}

/** The clickable capture field next to the given action label. */
function keyField(browser: WebdriverIO.Browser, action: string): ChainablePromiseElement {
    return browser.$(`//p[text()="${action}"]/following-sibling::div[1]/div[1]`);
}

function keyFieldLabel(browser: WebdriverIO.Browser, action: string): ChainablePromiseElement {
    return browser.$(`//p[text()="${action}"]/following-sibling::div[1]/div[1]/p`);
}

/**
 * Renders the Wayland layout (desktop-managed keys in grey, the System
 * Shortcuts button) on whatever platform is running the suite. The pixels
 * come from the real Wayland code path; the platform under them does not, so
 * these images show layout, not portal behavior.
 */
async function applyWaylandMocks(instanceName: string): Promise<void> {
    await mockCommand(instanceName, "app_platform_capabilities", {resolve: WAYLAND_CAPABILITIES});
    // A desktop that has its own Radio PTT shortcut assigned, so the Transmit
    // Config shows a radio key of its own rather than falling back to the
    // call key.
    await mockCommand(instanceName, "keybinds_is_portal_shortcut_bound", {resolve: true});
    await mockExternalBindings(instanceName, EXTERNAL_BINDINGS);
    await refetchCapabilities(instanceName);
}

/**
 * Mocks the per-action lookup of desktop-managed shortcuts. Unlike
 * mockCommand this one reads the invoke arguments, so each field can show a
 * different key.
 */
async function mockExternalBindings(
    instanceName: string,
    bindings: Record<string, string>,
): Promise<void> {
    await tauriApi(instanceName).execute((_tauri, map: Record<string, string>) => {
        type MockRegistry = Record<string, (args?: Record<string, unknown>) => unknown>;
        const w = window as Window & {__wdio_mocks__?: MockRegistry};
        w.__wdio_mocks__ = w.__wdio_mocks__ ?? {};
        w.__wdio_mocks__["keybinds_get_external_binding"] = args =>
            Promise.resolve(map[String(args?.keybind)] ?? null);
    }, bindings);
}

/** Re-runs the capability fetch, which otherwise only happens on mount. */
async function refetchCapabilities(instanceName: string): Promise<void> {
    await tauriApi(instanceName).execute(() => {
        type Hooks = {refetchCapabilities: () => Promise<void>};
        const w = window as Window & {__vacs_e2e__?: Hooks};
        if (w.__vacs_e2e__ === undefined) throw new Error("E2E hooks are not installed");
        void w.__vacs_e2e__.refetchCapabilities();
    });
}

async function activeCallId(instanceName: string): Promise<string | null> {
    return (await tauriApi(instanceName).execute(() => {
        type Hooks = {activeCallId: () => string | null};
        const w = window as Window & {__vacs_e2e__?: Hooks};
        if (w.__vacs_e2e__ === undefined) throw new Error("E2E hooks are not installed");
        return w.__vacs_e2e__.activeCallId();
    })) as string | null;
}

async function emitEvent(instanceName: string, event: string, payload: unknown): Promise<void> {
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
