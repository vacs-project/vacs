import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {click, getClient, selectOption} from "../helpers/browser.ts";

const CID_A = "10000004";

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

describe("Settings", () => {
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
});
