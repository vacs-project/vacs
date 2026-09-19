import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {
    callQueueSlot,
    click,
    clickSvg,
    getClient,
    mockCommandOn,
    ringSoundField,
    ringSoundReset,
    waitForCallColor,
    waitForRingSound,
} from "../helpers/browser.ts";
import {REMOTE_ADDR, setRemoteEnabled} from "../helpers/remote.ts";
import {RING_SOUND_NAME, writeRingSound} from "../helpers/ring-sound.ts";
import {SignalingTestClient} from "../helpers/signaling-client.ts";

const APP_CID = "10000004";
// A caller without a datafeed controller keeps its CID as display name.
const CALLER_CID = "10000005";

const BUILT_IN_CHIME = "Built-in chime";

/** Toggles the settings menu, which also closes any open settings page. */
async function toggleSettings(browser: WebdriverIO.Browser): Promise<void> {
    const settingsButton = await browser.$('//button[.//img[@alt="Settings"]]');
    await settingsButton.waitForDisplayed();
    await click(browser, settingsButton);
}

async function openCallSettings(browser: WebdriverIO.Browser): Promise<void> {
    await toggleSettings(browser);
    const callButton = await browser.$('//button[./p[text()="Call"]]');
    await callButton.waitForDisplayed();
    await click(browser, callButton);
    await ringSoundField(browser, "Ring").waitForDisplayed();
}

describe("Remote Control", () => {
    let caller: SignalingTestClient | undefined;
    let ringSound = "";

    before(() => {
        ringSound = writeRingSound();
    });

    beforeEach(async () => {
        await resetMockState();
        await restartApps();

        const clientA = getClient("clientA");
        await loginAndConnect(clientA, APP_CID);
        await setRemoteEnabled(clientA, true);

        // The second app instance doubles as the "remote browser": the page
        // served by the remote server has no Tauri IPC access, so it uses
        // the real remote WebSocket transport like any external browser.
        await getClient("remoteBrowser").url(`http://${REMOTE_ADDR}/`);
    });

    afterEach(() => {
        caller?.disconnect();
        caller = undefined;
    });

    it("should mirror the session and control calls from the remote browser", async () => {
        const clientA = getClient("clientA");
        const remoteBrowser = getClient("remoteBrowser");

        // The remote page hydrates into the connected session instead of
        // showing the login or connect pages.
        const endButton = await remoteBrowser.$("button=END");
        await endButton.waitForDisplayed();
        const connectButton = await remoteBrowser.$("button=Connect");
        await connectButton.waitForDisplayed({reverse: true});

        // An incoming call shows up on both the native and the remote UI.
        caller = await SignalingTestClient.connect(CALLER_CID);
        const callId = caller.invite(APP_CID);
        const answerKeyA = callQueueSlot(clientA, CALLER_CID);
        const answerKeyB = callQueueSlot(remoteBrowser, CALLER_CID);
        await answerKeyA.waitForDisplayed();
        await answerKeyB.waitForDisplayed();

        // Accepting from the remote browser drives the native client.
        await click(remoteBrowser, answerKeyB);
        await caller.waitForMessage(
            msg =>
                msg.type === "callUpdate" &&
                msg.callId === callId &&
                msg.joinedParticipants?.[APP_CID] !== undefined,
        );
        await waitForCallColor(clientA, callQueueSlot(clientA, CALLER_CID), {active: true});
        await waitForCallColor(remoteBrowser, callQueueSlot(remoteBrowser, CALLER_CID), {
            active: true,
        });

        // Ending from the remote browser clears the call everywhere.
        await click(remoteBrowser, endButton);
        await caller.waitForMessage(msg => msg.type === "callEnd" && msg.callId === callId);
        await answerKeyA.waitForDisplayed({reverse: true});
        await answerKeyB.waitForDisplayed({reverse: true});
    });

    it("should surface a disconnect overlay when the remote server goes away", async () => {
        const clientA = getClient("clientA");
        const remoteBrowser = getClient("remoteBrowser");

        const endButton = await remoteBrowser.$("button=END");
        await endButton.waitForDisplayed();

        // Disabling the remote server drops the remote transport. Open
        // WebSocket connections are not force-closed, so the page only
        // notices via its ping/pong timeout (up to ~10s).
        await setRemoteEnabled(clientA, false);
        const overlayTitle = await remoteBrowser.$("p=Remote disconnected");
        await overlayTitle.waitForDisplayed({timeout: 20000});

        // Re-enabling lets the remote page reconnect automatically.
        await setRemoteEnabled(clientA, true);
        await overlayTitle.waitForDisplayed({reverse: true, timeout: 15000});
        await endButton.waitForDisplayed();
    });

    it("shows the app's custom ring sound in the remote browser and resets it from there", async () => {
        const clientA = getClient("clientA");
        const remoteBrowser = getClient("remoteBrowser");

        // Not mockCommand: this config runs without @wdio/tauri-service, so
        // the app instance has no browser.tauri to install the mock through.
        await mockCommandOn(clientA, "audio_pick_ring_sound", {resolve: ringSound});

        await openCallSettings(clientA);
        await click(clientA, ringSoundField(clientA, "Ring"));
        await waitForRingSound(clientA, "Ring", RING_SOUND_NAME);
        // Closes the Call page, so the assertion at the end runs against
        // fields that fetched their state after the reset.
        await toggleSettings(clientA);

        await openCallSettings(remoteBrowser);
        await waitForRingSound(remoteBrowser, "Ring", RING_SOUND_NAME);

        // Picking a file needs the app's native dialog, so the field is inert
        // in a browser session. Deliberately not clicked: were it live, the
        // click would open a modal dialog on the app that nothing closes.
        const classes = (await ringSoundField(remoteBrowser, "Ring").getAttribute("class")) ?? "";
        if (!classes.includes("cursor-not-allowed")) {
            throw new Error("The ring sound field is not inert in the remote browser");
        }

        await clickSvg(remoteBrowser, ringSoundReset(remoteBrowser, "Ring"));
        await waitForRingSound(remoteBrowser, "Ring", BUILT_IN_CHIME);

        await openCallSettings(clientA);
        await waitForRingSound(clientA, "Ring", BUILT_IN_CHIME);
    });
});
