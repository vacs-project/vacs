import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {click, getClient, mockCommand} from "../helpers/browser.ts";

const CID_A = "10000004";

// Shape of the backend's FrontendError, which the error overlay expects.
const UPDATE_ERROR = {
    title: "Update failed",
    detail: "Injected by the E2E suite",
    isNonCritical: true,
};

async function openSettings(browser: WebdriverIO.Browser): Promise<void> {
    const settingsButton = await browser.$('//button[.//img[@alt="Settings"]]');
    await settingsButton.waitForDisplayed();
    await click(browser, settingsButton);
}

async function checkForUpdates(browser: WebdriverIO.Browser): Promise<void> {
    const checkButton = await browser.$("button*=Check for");
    await checkButton.waitForDisplayed();
    await click(browser, checkButton);
}

describe("Update Flow", () => {
    beforeEach(async () => {
        await resetMockState();
        await restartApps();

        await loginAndConnect(getClient("clientA"), CID_A);
    });

    it("should report when no update is available", async () => {
        const clientA = getClient("clientA");

        await mockCommand("clientA", "app_check_for_update", {
            resolve: {currentVersion: "2.5.1", required: false},
        });

        await openSettings(clientA);
        await checkForUpdates(clientA);

        await clientA.$("button*=No Update").waitForDisplayed();
    });

    it("should surface a failing optional update install", async () => {
        const clientA = getClient("clientA");

        await mockCommand("clientA", "app_check_for_update", {
            resolve: {
                currentVersion: "2.5.1",
                newVersion: "99.0.0",
                required: false,
            },
        });
        await mockCommand("clientA", "app_update", {reject: UPDATE_ERROR});

        await openSettings(clientA);
        await checkForUpdates(clientA);

        // An available update turns the button into the install action.
        const installButton = await clientA.$("button*=Update &");
        await installButton.waitForDisplayed();
        await click(clientA, installButton);

        // The failed install surfaces in the error overlay and the download
        // dialog does not stay up.
        await clientA.$(`p=${UPDATE_ERROR.title}`).waitForDisplayed();
        await clientA.$("p=Updating...").waitForDisplayed({reverse: true});
    });

    it("should keep insisting on a mandatory update when the install fails", async () => {
        const clientA = getClient("clientA");

        await mockCommand("clientA", "app_check_for_update", {
            resolve: {
                currentVersion: "2.5.1",
                newVersion: "99.0.0",
                required: true,
            },
        });
        await mockCommand("clientA", "app_update", {reject: UPDATE_ERROR});

        await openSettings(clientA);
        await checkForUpdates(clientA);

        // A required update opens the blocking dialog.
        const dialogTitle = () => clientA.$("p=Mandatory update");
        await dialogTitle().waitForDisplayed();

        // A failed install falls back to the dialog instead of letting the
        // user through.
        await click(clientA, await clientA.$("button=Update"));
        await clientA.$(`p=${UPDATE_ERROR.title}`).waitForDisplayed();
        await dialogTitle().waitForDisplayed();
    });
});
