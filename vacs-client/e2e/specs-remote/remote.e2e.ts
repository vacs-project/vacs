import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {callQueueSlot, click, getClient, waitForCallColor} from "../helpers/browser.ts";
import {SignalingTestClient} from "../helpers/signaling-client.ts";

const APP_CID = "10000004";
// A caller without a datafeed controller keeps its CID as display name.
const CALLER_CID = "10000005";
const REMOTE_ADDR = "127.0.0.1:9610";

/**
 * Enables the remote control server on the given app instance. The remote
 * frontend is then served at http://REMOTE_ADDR/.
 */
async function setRemoteEnabled(browser: WebdriverIO.Browser, enabled: boolean): Promise<void> {
    const result = await browser.execute(
        async (addr: string, on: boolean) => {
            try {
                await window.__TAURI_INTERNALS__.invoke("remote_set_config", {
                    remoteConfig: {enabled: on, listenAddr: addr, serveFrontend: true},
                });
                return {ok: true as const};
            } catch (e) {
                return {ok: false as const, error: String(e)};
            }
        },
        REMOTE_ADDR,
        enabled,
    );
    if (!result.ok) {
        throw new Error(`remote_set_config failed: ${result.error}`);
    }
}

describe("Remote Control", () => {
    let caller: SignalingTestClient | undefined;

    beforeEach(async () => {
        await resetMockState();
        await multiRemoteBrowser.reloadSession();

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
        await caller.waitForMessage(msg => msg.type === "callAccept" && msg.callId === callId);
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
});
