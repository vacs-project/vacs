import type {ChainablePromiseElement} from "webdriverio";
import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnectAs, resetMockState} from "../helpers/auth.ts";
import {click, getClient, mockCommand, selectOption, tauriApi} from "../helpers/browser.ts";
import {annotate, clearAnnotations} from "../helpers/annotate.ts";
import {
    applyFixtures,
    CID_A,
    openSettings,
    openSettingsPage,
    POSITION_A,
    refetchCapabilities,
    subPage,
} from "../helpers/docs.ts";
import {captureElement} from "../helpers/screenshot.ts";

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

// A Wayland desktop whose portal has no GlobalShortcuts interface (GNOME
// before 47, older Plasma): the keybind pages show a notice and fall back to
// joystick-only capture.
const WAYLAND_NO_PORTAL_CAPABILITIES = {...WAYLAND_CAPABILITIES, keybindListener: false};

// Keys the desktop environment would report for the portal shortcuts. Distinct
// per action so the Wayland images do not show identical fields.
const EXTERNAL_BINDINGS = {
    AcceptCall: "Ctrl+Alt+A",
    EndCall: "Ctrl+Alt+E",
    ToggleRadioPrio: "Ctrl+Alt+R",
    SayAgain: "Ctrl+Alt+S",
    PushToTalk: "Ctrl+Alt+T",
    PushToMute: "Ctrl+Alt+M",
    RadioPushToTalk: "Ctrl+Alt+P",
};

describe("Documentation screenshots: transmit and hotkeys", function () {
    this.timeout(120_000);

    beforeEach(async () => {
        await resetMockState();
        await restartApps();
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
        // On the last row, because a badge below any other one lands on the
        // row underneath it and would read as marking that one.
        await annotate(clientA, [
            {target: keyFieldSelector("SAY AGAIN"), badge: 1, place: "bottom-left", box: false},
            {target: removeButtonSelector("SAY AGAIN"), badge: 2, place: "below", box: false},
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

    it("captures the Hotkeys Config on Wayland without a shortcuts portal", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");
        await mockCommand("clientA", "app_platform_capabilities", {
            resolve: WAYLAND_NO_PORTAL_CAPABILITIES,
        });
        await refetchCapabilities("clientA");

        await openSettings(clientA);
        await openSettingsPage(clientA, "Hotkeys");
        const hotkeys = subPage(clientA, "Hotkeys Config");
        await hotkeys.waitForDisplayed();
        await clientA
            .$('//p[contains(., "Keyboard shortcuts are unavailable")]')
            .waitForDisplayed();

        await captureElement(clientA, hotkeys, "settings/HotkeysConfigPage-wayland-no-portal.png");
    });
});

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
    // The field attaches its key listener in an effect after the click
    // re-renders it; a keydown dispatched before that is lost.
    await browser.waitUntil(
        async () => (await browser.$(`${fieldSelector}/p`).getText()).startsWith("Press"),
        {timeoutMsg: "Capture field did not start capturing"},
    );
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
