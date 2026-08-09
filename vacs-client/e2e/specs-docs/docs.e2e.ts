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
import {annotate, clearAnnotations} from "../helpers/annotate.ts";
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

// The keybind pages render differently per platform, and the capture host's
// own session decides which variant you get: run this on a Wayland desktop
// and every image picks up the System Shortcuts button and desktop-managed
// key fields. Pin the platform so an image does not depend on where it was
// taken. X11 renders what Windows and macOS render, which is what the rest
// of the manual's images show.
const DESKTOP_CAPABILITIES = {
    alwaysOnTop: true,
    keybindListener: true,
    keybindEmitter: true,
    joystick: true,
    playback: true,
    platform: "LinuxX11",
};

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

    it("captures how the settings pages are opened", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        await openSettings(clientA);
        await clientA.$(SETTINGS_BUTTON).waitForDisplayed();

        // Both images show the same two steps: the settings button, then the
        // button for the page in question.
        for (const page of ["Transmit", "Hotkeys"] as const) {
            await annotate(clientA, [
                {target: SETTINGS_BUTTON, badge: 1, place: "top-left"},
                {target: settingsPageButtonSelector(page), badge: 2, place: "top-right"},
            ]);
            await captureWindow(clientA, `settings/${page}Config.png`);
            await clearAnnotations(clientA);
        }
    });

    it("captures the Hotkeys Config", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        await openSettings(clientA);
        await openSettingsPage(clientA, "Hotkeys");
        const dialog = subPage(clientA, "Hotkeys Config");
        await dialog.waitForDisplayed();

        // The page walks through assigning and clearing a binding; the
        // callouts mark the two controls that do it.
        // Badges only: the field and its clear button sit next to each other,
        // so boxes would collide, and the row is unambiguous without them.
        await annotate(clientA, [
            {
                target: keyFieldSelector("Toggle RADIO PRIO"),
                badge: 1,
                place: "bottom-left",
                box: false,
            },
            {
                target: removeButtonSelector("Toggle RADIO PRIO"),
                badge: 2,
                place: "below",
                box: false,
            },
        ]);
        await captureElement(clientA, dialog, "settings/HotkeysConfigPage.png");
        await clearAnnotations(clientA);
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

    it("captures the Transmit Config with Voice activation and no radio", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        const transmit = await openTransmitConfig(clientA);
        await captureElement(clientA, transmit, "settings/Transmit-VoiceActivation-None.png");
    });

    it("captures the Transmit Config with Voice activation and TrackAudio", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        const transmit = await openTransmitConfig(clientA);
        await selectRadioIntegration(clientA, "TrackAudio");

        // Voice activation has no call key to fall back to, so the radio key
        // field stays unbound until one is captured for it.
        await captureElement(clientA, transmit, "settings/Transmit-VoiceActivation-TrackAudio.png");
    });

    it("captures the Transmit Config with the radio on the call PTT key", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        const transmit = await openTransmitConfig(clientA);
        await selectCallMicMode(clientA, "PushToTalk");
        await bindKey(clientA, CALL_KEY_FIELD, "ControlLeft");
        await selectRadioIntegration(clientA, "TrackAudio");

        // With no radio key of its own, the field shows the call key as a
        // grey placeholder.
        await captureElement(clientA, transmit, "settings/Transmit-SamePTT-TrackAudio.png");
    });

    it("captures the Transmit Config with a separate radio PTT key", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        const transmit = await openTransmitConfig(clientA);
        await selectCallMicMode(clientA, "PushToTalk");
        await bindKey(clientA, CALL_KEY_FIELD, "ControlLeft");
        await selectRadioIntegration(clientA, "TrackAudio");
        await bindKey(clientA, RADIO_KEY_FIELD, "AltRight");

        await captureElement(clientA, transmit, "settings/Transmit-DifferentPTT-TrackAudio.png");
    });

    it("captures the Transmit Config with Push-to-mute", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        const transmit = await openTransmitConfig(clientA);
        await selectCallMicMode(clientA, "PushToMute");
        await bindKey(clientA, CALL_KEY_FIELD, "AltRight");
        await selectRadioIntegration(clientA, "TrackAudio");

        // Push-to-mute forces the radio onto the call key, so its field is
        // locked to the same key.
        await captureElement(clientA, transmit, "settings/Transmit-PTM-TrackAudio.png");
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

        // Transmit dialog crops are named after the combination they show,
        // following the existing Transmit-<mic mode>-<integration> set.
        await captureElement(
            clientA,
            transmit,
            "settings/Transmit-DifferentPTT-TrackAudio-wayland.png",
        );
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

        // Wait out the real negotiation first: a call-connected event arriving
        // after the degrade event would put the call back to connected and
        // take the icon away again mid-capture.
        const indicator = clientA.$(
            '//div[contains(@title, "Click to switch to")]//div[contains(@class, "rounded-full")]',
        );
        await clientA.waitUntil(
            async () => ((await indicator.getAttribute("class")) ?? "").includes("bg-green"),
            {timeoutMsg: "Call did not reach the connected state"},
        );

        // A real one-way-audio situation needs a broken network path between
        // two hosts; the event the media watchdog would emit for it does not.
        const callId = await activeCallId("clientA");
        if (callId === null) throw new Error("No active call to degrade");
        await emitEvent("clientA", "webrtc:call-degraded", callId);

        await clientA.$('img[alt="No incoming audio"]').waitForDisplayed();

        await captureWindow(clientA, "troubleshooting/degraded-call.png");

        // The annotated variant marks the two symptoms the page lists, in the
        // order the prose lists them. The callouts are anchored to the
        // elements, so they stay on the right thing when the layout moves.
        await annotate(clientA, [
            {
                target: '//div[contains(@title, "Click to switch to")]//div[contains(@class, "rounded-full")]',
                badge: 1,
                // The indicator sits in the window's top-left corner, so the
                // badge goes below it: the other corners cover the clock.
                place: "bottom-right",
            },
            {
                target: '//div[img[@alt="No incoming audio"]]',
                badge: 2,
                place: "top-left",
            },
        ]);
        await captureWindow(clientA, "troubleshooting/degraded-call-annotated.png");
        await clearAnnotations(clientA);
    });
});

/**
 * Pins everything in the window that would otherwise differ per run: the
 * clock, the version in the header, and the platform the UI renders for.
 */
async function applyFixtures(browser: WebdriverIO.Browser, instanceName: string): Promise<void> {
    await mockCommand(instanceName, "app_platform_capabilities", {resolve: DESKTOP_CAPABILITIES});
    await refetchCapabilities(instanceName);
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

/** The two key capture fields of the Transmit Config, each next to its select. */
const CALL_KEY_FIELD = '//select[@name="keybind-mode"]/following-sibling::div[1]/div[1]';
const RADIO_KEY_FIELD = '//select[@name="radio-integration"]/following-sibling::div[1]/div[1]';

async function openTransmitConfig(browser: WebdriverIO.Browser): Promise<ChainablePromiseElement> {
    await openSettings(browser);
    await openSettingsPage(browser, "Transmit");
    const transmit = subPage(browser, "Transmit Config");
    await transmit.waitForDisplayed();
    return transmit;
}

async function selectCallMicMode(browser: WebdriverIO.Browser, mode: string): Promise<void> {
    await selectOption(browser, 'select[name="keybind-mode"]', mode);
    await browser.waitUntil(
        async () => (await browser.$('//select[@name="keybind-mode"]').getValue()) === mode,
        {timeoutMsg: `Call mic mode did not switch to ${mode}`},
    );
}

async function selectRadioIntegration(
    browser: WebdriverIO.Browser,
    integration: string,
): Promise<void> {
    await selectOption(browser, 'select[name="radio-integration"]', integration);
    await browser.waitUntil(
        async () =>
            (await browser.$('//select[@name="radio-integration"]').getValue()) === integration,
        {timeoutMsg: `Radio integration did not switch to ${integration}`},
    );
}

/**
 * Binds a keyboard key in a capture field. The keypress is dispatched into
 * the page rather than sent through WebDriver, the same reason clicks are:
 * the capture listens on `document`, and a synthetic event carries the code,
 * which is all the handler reads.
 */
async function bindKey(
    browser: WebdriverIO.Browser,
    fieldSelector: string,
    code: string,
): Promise<void> {
    await click(browser, browser.$(fieldSelector));
    await browser.execute((keyCode: string) => {
        document.dispatchEvent(
            new KeyboardEvent("keydown", {code: keyCode, key: keyCode, bubbles: true}),
        );
    }, code);
    await browser.waitUntil(
        async () => (await browser.$(`${fieldSelector}/p`).getText()) === code,
        {timeoutMsg: `Key ${code} was not bound`},
    );
}

/** The wrench in the window header that opens the settings page. */
const SETTINGS_BUTTON = '//button[.//img[@alt="Settings"]]';

async function openSettings(browser: WebdriverIO.Browser): Promise<void> {
    const settingsButton = await browser.$(SETTINGS_BUTTON);
    await settingsButton.waitForDisplayed();
    await click(browser, settingsButton);
}

/**
 * A settings page button. Exact text match: a substring match on "Call" would
 * also hit the call controls on the page behind the settings menu.
 */
function settingsPageButtonSelector(label: string): string {
    return `//button[./p[text()="${label}"]]`;
}

async function openSettingsPage(browser: WebdriverIO.Browser, label: string): Promise<void> {
    const button = await browser.$(settingsPageButtonSelector(label));
    await button.waitForDisplayed();
    await click(browser, button);
}

/** The settings sub page dialog carrying the given title. */
function subPage(browser: WebdriverIO.Browser, title: string): ChainablePromiseElement {
    return browser.$(`//div[./p[text()="${title}"]]`);
}

/** The clickable capture field next to the given action label. */
function keyField(browser: WebdriverIO.Browser, action: string): ChainablePromiseElement {
    return browser.$(keyFieldSelector(action));
}

function keyFieldSelector(action: string): string {
    return `//p[text()="${action}"]/following-sibling::div[1]/div[1]`;
}

/** The x that clears the binding, at the right end of the same row. */
function removeButtonSelector(action: string): string {
    return `//p[text()="${action}"]/following-sibling::div[1]/*[name()="svg"]`;
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
