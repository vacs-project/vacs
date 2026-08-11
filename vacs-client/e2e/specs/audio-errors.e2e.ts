import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {click, getClient, mockCommand, selectOption, unmockCommand} from "../helpers/browser.ts";

const CID_A = "10000004";

// Shape of the backend's FrontendError, which the error overlay expects.
const AUDIO_ERROR = {
    title: "Audio backend error",
    detail: "Injected by the E2E suite",
    isNonCritical: true,
};

async function openSettings(browser: WebdriverIO.Browser): Promise<void> {
    const settingsButton = await browser.$('//button[.//img[@alt="Settings"]]');
    await settingsButton.waitForDisplayed();
    await click(browser, settingsButton);
}

describe("Audio Errors", () => {
    beforeEach(async () => {
        await resetMockState();
        await restartApps();

        await loginAndConnect(getClient("clientA"), CID_A);
    });

    it("should surface a failing device switch and roll back the selection", async () => {
        const clientA = getClient("clientA");
        await openSettings(clientA);

        // The speaker device starts out unset.
        const speakerSelect = await clientA.$('select[name="Speaker"]');
        await speakerSelect.waitForDisplayed();
        const initialDevice = await speakerSelect.getValue();

        await mockCommand("clientA", "audio_set_device", {reject: AUDIO_ERROR});

        await selectOption(clientA, 'select[name="Speaker"]', "Mock Speaker");

        // The failure surfaces in the error overlay and the optimistic
        // selection is rolled back.
        const errorTitle = () => clientA.$(`p=${AUDIO_ERROR.title}`);
        await errorTitle().waitForDisplayed();
        await click(clientA, await errorTitle());
        await errorTitle().waitForDisplayed({reverse: true});

        if ((await speakerSelect.getValue()) !== initialDevice) {
            throw new Error("Failed device switch was not rolled back");
        }

        // Once the backend recovers, switching works again.
        await unmockCommand("clientA", "audio_set_device");
        await selectOption(clientA, 'select[name="Speaker"]', "Mock Speaker");
        await clientA.pause(500);
        if ((await speakerSelect.getValue()) !== "Mock Speaker") {
            throw new Error("Device switch after recovery was not applied");
        }
    });

    it("should surface a failing device enumeration", async () => {
        const clientA = getClient("clientA");

        await mockCommand("clientA", "audio_get_devices", {reject: AUDIO_ERROR});

        // Opening the settings page triggers the device list fetch.
        await openSettings(clientA);
        await clientA.$(`p=${AUDIO_ERROR.title}`).waitForDisplayed();
    });
});
