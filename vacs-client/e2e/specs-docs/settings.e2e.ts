import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnectAs, resetMockState} from "../helpers/auth.ts";
import {
    click,
    getClient,
    mockCommand,
    ringSoundField,
    waitForRingSound,
} from "../helpers/browser.ts";
import {annotate, clearAnnotations} from "../helpers/annotate.ts";
import {
    applyFixtures,
    CID_A,
    NEXT_VERSION,
    openSettings,
    openSettingsPage,
    POSITION_A,
    SETTINGS_BUTTON,
    settingsPageButtonSelector,
    subPage,
} from "../helpers/docs.ts";
import {writeRingSound} from "../helpers/ring-sound.ts";
import {captureElement, captureRect, captureWindow, unionRect} from "../helpers/screenshot.ts";

const RING_FILE = "ring.wav";

/** The volume sliders: the Call and Settings Touch Panel columns of the settings page. */
const VOLUME_COLUMNS = [
    '//div[contains(@class, "w-64")][./p[text()="Call"]]',
    '//div[./p[text()="Settings Touch Panel"]]',
];
/** The device selects, from their heading to the last select. */
const DEVICE_SECTION = ['//p[text()="Devices"]', '//p[text()="Devices"]/following-sibling::div[1]'];
const MISC_SECTION = '//div[./p[text()="Miscellaneous"]]';
const SESSION_BUTTONS = '//button[.//p[text()="Disconnect"]]/..';
const UPDATE_BUTTON = '//button[.//p[contains(., "Restart")]]';

/** The row of the Call Config carrying the given setting label. */
function callConfigRowSelector(label: string): string {
    return `//div[./label[text()="${label}"]]`;
}

describe("Documentation screenshots: settings", function () {
    this.timeout(120_000);

    beforeEach(async () => {
        await resetMockState();
        await restartApps();
    });

    it("captures the settings page and how its sub pages are opened", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        await openSettings(clientA);
        await clientA.$(MISC_SECTION).waitForDisplayed();

        await annotate(clientA, [{target: SETTINGS_BUTTON, badge: 1, place: "top-left"}]);
        await captureWindow(clientA, "settings/overview.png");
        await clearAnnotations(clientA);

        // Every image shows the same two steps: the settings button, then the
        // button for the page in question.
        for (const page of ["Transmit", "Hotkeys", "Call", "Advanced"] as const) {
            await annotate(clientA, [
                {target: SETTINGS_BUTTON, badge: 1, place: "top-left"},
                {target: settingsPageButtonSelector(page), badge: 2, place: "top-right"},
            ]);
            await captureWindow(clientA, `settings/${page}Config.png`);
            await clearAnnotations(clientA);
        }

        await annotate(clientA, [
            {target: SETTINGS_BUTTON, badge: 1, place: "top-left"},
            {target: MISC_SECTION, badge: 2, place: "top-left"},
            {target: SESSION_BUTTONS, badge: 3, place: "top-left"},
        ]);
        await captureWindow(clientA, "settings/misc.png");
        await clearAnnotations(clientA);

        await captureRect(
            clientA,
            "settings/AudioDevices.png",
            await unionRect(clientA, DEVICE_SECTION),
        );
        await captureRect(
            clientA,
            "settings/VolumeControls.png",
            await unionRect(clientA, VOLUME_COLUMNS),
        );
    });

    it("captures the Advanced Config", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        // The mock audio backend calls its host "MockHost"; the manual shows
        // the default host of Windows, where most users are.
        await mockCommand("clientA", "audio_get_hosts", {
            resolve: {selected: "Wasapi", all: ["Wasapi", "Asio"]},
        });

        await openSettings(clientA);
        await openSettingsPage(clientA, "Advanced");
        const dialog = subPage(clientA, "Advanced");
        await dialog.waitForDisplayed();
        await clientA.waitUntil(
            async () => (await clientA.$('select[name="audio-host"]').getValue()) === "Wasapi",
            {timeoutMsg: "The audio host select did not show the mocked host"},
        );

        await captureElement(clientA, dialog, "settings/AdvancedConfigPage.png");
    });

    it("captures the update button on the settings page", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA", {newVersion: NEXT_VERSION});

        await openSettings(clientA);
        await clientA.$(UPDATE_BUTTON).waitForDisplayed();

        await annotate(clientA, [
            {target: SETTINGS_BUTTON, badge: 1, place: "top-left"},
            {target: UPDATE_BUTTON, badge: 2, place: "top-left"},
        ]);
        await captureWindow(clientA, "getting-started/update_available_settings.png");
        await clearAnnotations(clientA);
    });

    it("captures the Call Config", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        await openSettings(clientA);
        await openSettingsPage(clientA, "Call");
        const dialog = subPage(clientA, "Call Config");
        await dialog.waitForDisplayed();

        // The two conference sounds, in the order the page lists them.
        await annotate(clientA, [
            {
                target: callConfigRowSelector("Play participant joined sound"),
                badge: 1,
                place: "left",
            },
            {
                target: callConfigRowSelector("Play participant left sound"),
                badge: 2,
                place: "left",
            },
        ]);
        // Padding, so the badges outside the rows' left edge stay in frame.
        await captureElement(clientA, dialog, "settings/CallConfigPage.png", {padding: 18});
        await clearAnnotations(clientA);
    });

    it("captures the Call Config with a custom ring sound", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        // The file dialog is the only part that cannot run headless; the file
        // it returns is real, so the backend loads and accepts it.
        await mockCommand("clientA", "audio_pick_ring_sound", {resolve: writeRingSound(RING_FILE)});

        await openSettings(clientA);
        await openSettingsPage(clientA, "Call");
        const dialog = subPage(clientA, "Call Config");
        await dialog.waitForDisplayed();

        await click(clientA, ringSoundField(clientA, "Ring"));
        await waitForRingSound(clientA, "Ring", RING_FILE);
        await waitForRingSound(clientA, "Priority ring", "Built-in chime");

        // The section sits below the fold of the dialog's scroll area.
        await clientA.execute(
            pane => {
                pane.scrollTop = pane.scrollHeight;
            },
            await dialog.$("./div[contains(@class, 'overflow-auto')]"),
        );
        await captureElement(clientA, dialog, "settings/CallConfigRingSounds.png");
    });
});
