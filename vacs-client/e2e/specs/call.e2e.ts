import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {callQueueSlot, click, clientKey, getClient, waitForCallColor} from "../helpers/browser.ts";

const CID_A = "10000001";
const CID_B = "10000002";

/**
 * Opens the "OTHER" client group on the client page, which contains all
 * connected clients without a resolved VATSIM position (labeled by CID).
 */
async function openOtherClients(browser: WebdriverIO.Browser): Promise<void> {
    const group = await browser.$("button*=OTHER");
    await group.waitForDisplayed();
    await click(browser, group);
}

/**
 * Starts a call from the given browser to the client with the given CID by
 * clicking its client key.
 */
async function startCallTo(browser: WebdriverIO.Browser, targetCid: string): Promise<void> {
    await openOtherClients(browser);
    const key = clientKey(browser, targetCid);
    await key.waitForDisplayed();
    await click(browser, key);
}

describe("Call Flow", () => {
    beforeEach(async () => {
        await resetMockState();
        await multiRemoteBrowser.reloadSession();

        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        await loginAndConnect(clientA, CID_A);
        await loginAndConnect(clientB, CID_B);
    });

    it("should show an outgoing call on both sides and cancel it via END", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");

        await startCallTo(clientA, CID_B);

        // Caller sees the outgoing call in the call display slot, callee sees
        // an incoming answer key.
        const outgoingSlot = callQueueSlot(clientA, CID_B);
        await outgoingSlot.waitForDisplayed();
        const answerKey = callQueueSlot(clientB, CID_A);
        await answerKey.waitForDisplayed();

        const endButton = await clientA.$("button=END");
        await click(clientA, endButton);

        await outgoingSlot.waitForDisplayed({reverse: true});
        await answerKey.waitForDisplayed({reverse: true});
    });

    it("should establish a call on accept and end it from the callee", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");

        await startCallTo(clientA, CID_B);

        const answerKey = callQueueSlot(clientB, CID_A);
        await answerKey.waitForDisplayed();
        await click(clientB, answerKey);

        // Both sides show the active call: the caller's client key for the
        // callee and the callee's call display slot turn steady green.
        await waitForCallColor(clientA, clientKey(clientA, CID_B), {active: true});
        await waitForCallColor(clientB, callQueueSlot(clientB, CID_A), {active: true});

        // Give WebRTC a moment to negotiate, then verify the media connection
        // did not fail: no disconnected indicator and the call is still active.
        await clientA.pause(1500);
        const disconnectedIconA = await clientA.$('img[alt="Disconnected"]');
        await disconnectedIconA.waitForDisplayed({reverse: true});
        const disconnectedIconB = await clientB.$('img[alt="Disconnected"]');
        await disconnectedIconB.waitForDisplayed({reverse: true});
        await waitForCallColor(clientA, clientKey(clientA, CID_B), {active: true});

        // The callee ends the call by clicking the caller's client key.
        await openOtherClients(clientB);
        const callerKey = clientKey(clientB, CID_A);
        await callerKey.waitForDisplayed();
        await click(clientB, callerKey);

        await callQueueSlot(clientA, CID_B).waitForDisplayed({reverse: true});
        await callQueueSlot(clientB, CID_A).waitForDisplayed({reverse: true});
        await waitForCallColor(clientA, clientKey(clientA, CID_B), {active: false});
    });

    it("should place a call by CID via the dial pad", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");

        // Open the telephone menu and switch to the dial pad tab.
        const telephoneButton = await clientA.$('//button[.//img[@alt="Telephone"]]');
        await click(clientA, telephoneButton);
        const dialPadTab = await clientA.$("button*=Dial");
        await dialPadTab.waitForDisplayed();
        await click(clientA, dialPadTab);

        // Dial the callee's CID digit by digit. Digit keys 2-9 also contain
        // their letter row (e.g. "2 ABC"), so match on the leading digit of
        // the dial pad buttons (class text-lg) instead of exact text.
        for (const digit of CID_B) {
            const digitButton = await clientA.$(
                `//button[contains(@class, "text-lg")][starts-with(normalize-space(.), "${digit}")]`,
            );
            await click(clientA, digitButton);
        }
        const callButton = await clientA.$("button=Call");
        await click(clientA, callButton);

        const answerKey = callQueueSlot(clientB, CID_A);
        await answerKey.waitForDisplayed();
        await click(clientB, answerKey);

        await waitForCallColor(clientB, callQueueSlot(clientB, CID_A), {active: true});

        // The caller ends the call via the global END button.
        const endButton = await clientA.$("button=END");
        await click(clientA, endButton);

        await callQueueSlot(clientA, CID_B).waitForDisplayed({reverse: true});
        await callQueueSlot(clientB, CID_A).waitForDisplayed({reverse: true});
    });

    it("should clear the active call when the peer disconnects", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");

        await startCallTo(clientA, CID_B);

        const answerKey = callQueueSlot(clientB, CID_A);
        await answerKey.waitForDisplayed();
        await click(clientB, answerKey);

        await waitForCallColor(clientA, clientKey(clientA, CID_B), {active: true});

        // The callee disconnects from the signaling server entirely.
        await clientB.execute(async () => {
            await window.__TAURI_INTERNALS__.invoke("signaling_disconnect");
        });

        // The caller's call ends and the callee disappears from the client list.
        await callQueueSlot(clientA, CID_B).waitForDisplayed({reverse: true});
        await clientKey(clientA, CID_B).waitForDisplayed({reverse: true});
    });
});
