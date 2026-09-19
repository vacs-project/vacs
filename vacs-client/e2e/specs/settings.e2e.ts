import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {
    click,
    clickSvg,
    getClient,
    mockCommand,
    ringSoundField,
    ringSoundReset,
    selectOption,
    waitForRingSound,
} from "../helpers/browser.ts";
import {RING_SOUND_NAME, writeInvalidRingSound, writeRingSound} from "../helpers/ring-sound.ts";

const CID_A = "10000004";

const BUILT_IN_CHIME = "Built-in chime";

async function openSettings(browser: WebdriverIO.Browser): Promise<void> {
    const settingsButton = await browser.$('//button[.//img[@alt="Settings"]]');
    await settingsButton.waitForDisplayed();
    await click(browser, settingsButton);
}

async function openAdvancedSettings(browser: WebdriverIO.Browser): Promise<void> {
    await openSettings(browser);
    const advancedButton = await browser.$("button*=Advanced");
    await advancedButton.waitForDisplayed();
    await click(browser, advancedButton);
}

/** Opens the Call page of an already open settings menu. */
async function openCallSettings(browser: WebdriverIO.Browser): Promise<void> {
    const callButton = await browser.$('//button[./p[text()="Call"]]');
    await callButton.waitForDisplayed();
    await click(browser, callButton);
    await ringSoundField(browser, "Ring").waitForDisplayed();
}

/**
 * Reopens the Call page over the advanced page. The ring sound fields fetch
 * their state once, when they mount, so anything the backend changed while the
 * page was open is only visible after it has been through another page.
 */
async function reopenCallSettings(browser: WebdriverIO.Browser): Promise<void> {
    await click(browser, browser.$("button*=Advanced"));
    await browser.$('select[name="cpl-mode"]').waitForDisplayed();
    await openCallSettings(browser);
}

describe("Settings", () => {
    let ringSound = "";
    let invalidRingSound = "";

    before(() => {
        ringSound = writeRingSound();
        invalidRingSound = writeInvalidRingSound();
    });

    beforeEach(async () => {
        await resetMockState();
        await restartApps();

        await loginAndConnect(getClient("clientA"), CID_A);
    });

    it("should list and switch mock audio devices", async () => {
        const clientA = getClient("clientA");
        await openSettings(clientA);

        // The mock audio backend's devices appear in the device selects.
        const inputSelect = await clientA.$('select[name="Input"]');
        await inputSelect.waitForDisplayed();
        const inputOption = await clientA.$(
            '//select[@name="Input"]/option[text()="Mock Microphone"]',
        );
        await inputOption.waitForExist();
        const outputOption = await clientA.$(
            '//select[@name="Output"]/option[text()="Mock Speaker"]',
        );
        await outputOption.waitForExist();

        // Switching devices takes effect (and is not rolled back by a
        // failing backend call).
        await selectOption(clientA, 'select[name="Input"]', "Mock Microphone");
        await clientA.pause(500);
        if ((await inputSelect.getValue()) !== "Mock Microphone") {
            throw new Error("Input device selection was not applied");
        }

        const outputSelect = await clientA.$('select[name="Output"]');
        await selectOption(clientA, 'select[name="Output"]', "Mock Speaker");
        await clientA.pause(500);
        if ((await outputSelect.getValue()) !== "Mock Speaker") {
            throw new Error("Output device selection was not applied");
        }

        // Enabling the speaker device works as well.
        const speakerSelect = await clientA.$('select[name="Speaker"]');
        await selectOption(clientA, 'select[name="Speaker"]', "Mock Speaker");
        await clientA.pause(500);
        if ((await speakerSelect.getValue()) !== "Mock Speaker") {
            throw new Error("Speaker device selection was not applied");
        }
    });

    it("should switch the couple mode in the advanced settings", async () => {
        const clientA = getClient("clientA");
        await openAdvancedSettings(clientA);

        // The mock audio host is offered and couple mode can be changed.
        const hostOption = await clientA.$(
            '//select[@name="audio-host"]/option[text()="MockHost"]',
        );
        await hostOption.waitForExist();

        const cplSelect = await clientA.$('select[name="cpl-mode"]');
        await cplSelect.waitForDisplayed();
        await selectOption(clientA, 'select[name="cpl-mode"]', "Fast");
        await clientA.pause(500);
        if ((await cplSelect.getValue()) !== "Fast") {
            throw new Error("Couple mode selection was not applied");
        }
    });

    it("should show a disabled SAY AGAIN function key without a radio connection", async () => {
        const clientA = getClient("clientA");

        const sayAgainKey = await clientA.$("button*=SAY");
        await sayAgainKey.waitForDisplayed();

        // The harness has no TrackAudio mock, so no radio integration is
        // configured and the key can only render disabled.
        if (await sayAgainKey.isEnabled()) {
            throw new Error("SAY AGAIN is enabled without a radio connection");
        }

        if (await clientA.$("button*=PLC").isExisting()) {
            throw new Error("PLC LSP placeholder is still rendered next to SAY AGAIN");
        }

        await openAdvancedSettings(clientA);

        // Wait for the playback section itself, so the missing checkbox below
        // cannot just be a page that has not rendered yet.
        const playbackCheckbox = await clientA.$('input[name="playback-enabled"]');
        await playbackCheckbox.waitForDisplayed();
        if (await clientA.$('input[name="say-again-enabled"]').isExisting()) {
            throw new Error("The SAY AGAIN setting is still offered in the advanced settings");
        }
    });

    it("should cycle the clock display mode", async () => {
        const clientA = getClient("clientA");

        const clock = await clientA.$('//div[contains(@title, "Click to switch to")]');
        await clock.waitForDisplayed();
        const initialTitle = await clock.getAttribute("title");

        await click(clientA, clock);
        await clientA.waitUntil(async () => (await clock.getAttribute("title")) !== initialTitle, {
            timeoutMsg: "Clock mode did not change on click",
        });
    });

    it("applies a custom ring sound and resets it to the built-in chime", async () => {
        const clientA = getClient("clientA");
        // Only the file dialog is mocked; the real backend command still
        // decodes and validates the generated file.
        await mockCommand("clientA", "audio_pick_ring_sound", {resolve: ringSound});

        await openSettings(clientA);
        await openCallSettings(clientA);
        await waitForRingSound(clientA, "Ring", BUILT_IN_CHIME);

        await click(clientA, ringSoundField(clientA, "Ring"));
        await waitForRingSound(clientA, "Ring", RING_SOUND_NAME);
        if (await clientA.$("p=Ring sound error").isExisting()) {
            throw new Error("The backend rejected the generated ring sound");
        }

        await clickSvg(clientA, ringSoundReset(clientA, "Ring"));
        await waitForRingSound(clientA, "Ring", BUILT_IN_CHIME);
    });

    it("keeps the custom ring sound across an output device switch", async () => {
        const clientA = getClient("clientA");
        await mockCommand("clientA", "audio_pick_ring_sound", {resolve: ringSound});

        await openSettings(clientA);
        await openCallSettings(clientA);
        await click(clientA, ringSoundField(clientA, "Ring"));
        await waitForRingSound(clientA, "Ring", RING_SOUND_NAME);

        // The switch rebuilds the playback stream, which has to carry the
        // decoded clip over to the new one.
        const outputSelect = await clientA.$('select[name="Output"]');
        await selectOption(clientA, 'select[name="Output"]', "Mock Speaker");
        await clientA.waitUntil(async () => (await outputSelect.getValue()) === "Mock Speaker", {
            timeoutMsg: "Output device selection was not applied",
        });

        await reopenCallSettings(clientA);
        await waitForRingSound(clientA, "Ring", RING_SOUND_NAME);
        const classes = (await ringSoundField(clientA, "Ring").getAttribute("class")) ?? "";
        if (classes.includes("text-red-700")) {
            throw new Error("The ring sound is flagged unavailable after the device switch");
        }
    });

    it("rejects a file that is not a WAV and keeps the built-in chime", async () => {
        const clientA = getClient("clientA");
        await mockCommand("clientA", "audio_pick_ring_sound", {resolve: invalidRingSound});

        await openSettings(clientA);
        await openCallSettings(clientA);
        await click(clientA, ringSoundField(clientA, "Ring"));

        await clientA.$("p=Ring sound error").waitForDisplayed();
        await waitForRingSound(clientA, "Ring", BUILT_IN_CHIME);
    });
});
