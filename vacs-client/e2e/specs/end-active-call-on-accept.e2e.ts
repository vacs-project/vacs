import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {
    callDisplaySlot,
    callQueueSlot,
    click,
    clickTwice,
    expectNoModalOverlay,
    getClient,
    showClientKey,
    startCallTo,
    waitForCallColor,
    waitForCallDisplayOf,
    waitForConnectedCall,
    waitForOutgoingCall,
} from "../helpers/browser.ts";
import {type ServerMessage, SignalingTestClient} from "../helpers/signaling-client.ts";

const APP_CID = "10000004";
// Raw signaling clients: positionless CIDs, so their keys keep the CID as label
// and no controller has to be removed from the mock datafeed.
const ACTIVE_PEER_CID = "10000005";
const CALLER_CID = "10000006";
const SECOND_CALLER_CID = "10000007";

// Raw clients never answer the SDP, so the app tears an accepted call down at
// its 15s establishment timeout. Every check on an accepted call, this window
// included, has to fit inside that.
const NO_REACTION_WINDOW_MS = 2000;

const SETTINGS_BUTTON = '//button[.//img[@alt="Settings"]]';

type CallConfig = {endActiveCallOnAccept: boolean};

async function endActiveCallOnAccept(browser: WebdriverIO.Browser): Promise<boolean> {
    const config = await browser.execute(() =>
        window.__TAURI_INTERNALS__.invoke<CallConfig>("app_get_call_config"),
    );
    return config.endActiveCallOnAccept;
}

async function disableEndActiveCallOnAccept(browser: WebdriverIO.Browser): Promise<void> {
    const settingsButton = browser.$(SETTINGS_BUTTON);
    await settingsButton.waitForDisplayed();
    await click(browser, settingsButton);

    const callButton = browser.$('//button[./p[text()="Call"]]');
    await callButton.waitForDisplayed();
    await click(browser, callButton);

    const checkbox = browser.$('input[name="end-active-call-on-accept"]');
    await checkbox.waitForDisplayed();
    if (!(await checkbox.isSelected())) {
        throw new Error("End active call on accept is already off");
    }
    await click(browser, checkbox);
    // The click flips the checkbox before the backend stored anything, so ask the backend.
    await browser.waitUntil(async () => !(await endActiveCallOnAccept(browser)), {
        timeoutMsg: "The backend did not store End active call on accept as off",
    });

    // The settings button toggles, so the same click closes the menu again.
    await click(browser, browser.$(SETTINGS_BUTTON));
    await browser.$('//p[text()="Settings"]').waitForDisplayed({reverse: true});
}

const joinedByApp = (callId: string) => (msg: ServerMessage) =>
    msg.type === "callUpdate" &&
    msg.callId === callId &&
    msg.joinedParticipants?.[APP_CID] !== undefined;

const endedOrCancelled = (callId: string) => (msg: ServerMessage) =>
    (msg.type === "callEnd" || msg.type === "callCancelled") && msg.callId === callId;

describe("End Active Call On Accept", () => {
    let peers: SignalingTestClient[] = [];

    beforeEach(async () => {
        await resetMockState();
        await restartApps();

        await loginAndConnect(getClient("clientA"), APP_CID);
    });

    afterEach(() => {
        for (const peer of peers) {
            peer.disconnect();
        }
        peers = [];
    });

    async function connectPeers(...cids: string[]): Promise<SignalingTestClient[]> {
        const connected = await Promise.all(cids.map(cid => SignalingTestClient.connect(cid)));
        peers.push(...connected);
        return connected;
    }

    async function acceptCallFrom(
        app: WebdriverIO.Browser,
        peer: SignalingTestClient,
    ): Promise<string> {
        const callId = peer.invite(APP_CID);
        await callQueueSlot(app, peer.cid).waitForDisplayed();
        await click(app, callQueueSlot(app, peer.cid));
        await peer.waitForMessage(joinedByApp(callId));
        await waitForCallDisplayOf(app, peer.cid);
        await waitForCallColor(app, callDisplaySlot(app), {active: true});
        return callId;
    }

    async function ringIn(app: WebdriverIO.Browser, caller: SignalingTestClient): Promise<string> {
        const callId = caller.invite(APP_CID);
        await callQueueSlot(app, caller.cid).waitForDisplayed();
        return callId;
    }

    it("should end the active call and accept the clicked incoming call when enabled", async () => {
        const app = getClient("clientA");
        if (!(await endActiveCallOnAccept(app))) {
            throw new Error("End active call on accept is off by default");
        }
        const [activePeer, caller, secondCaller] = await connectPeers(
            ACTIVE_PEER_CID,
            CALLER_CID,
            SECOND_CALLER_CID,
        );

        const activeCallId = await acceptCallFrom(app, activePeer);

        const callerCallId = await ringIn(app, caller);
        await click(app, callQueueSlot(app, CALLER_CID));

        await activePeer.waitForMessage(endedOrCancelled(activeCallId));
        await caller.waitForMessage(joinedByApp(callerCallId));
        await waitForCallDisplayOf(app, CALLER_CID);
        await waitForCallColor(app, callDisplaySlot(app), {active: true});
        await callQueueSlot(app, ACTIVE_PEER_CID).waitForDisplayed({reverse: true});

        // A call that was itself swapped in must be replaceable as well.
        const secondCallId = await ringIn(app, secondCaller);
        await click(app, await showClientKey(app, SECOND_CALLER_CID));

        await caller.waitForMessage(endedOrCancelled(callerCallId));
        await secondCaller.waitForMessage(joinedByApp(secondCallId));
        await waitForCallDisplayOf(app, SECOND_CALLER_CID);
        await waitForCallColor(app, callDisplaySlot(app), {active: true});
    });

    it("should ignore a repeated accept for the call that was just swapped in", async () => {
        const app = getClient("clientA");
        const [activePeer, caller] = await connectPeers(ACTIVE_PEER_CID, CALLER_CID);

        const activeCallId = await acceptCallFrom(app, activePeer);
        const callerCallId = await ringIn(app, caller);

        // The answer key is not debounced, so a double click sends two accepts.
        await clickTwice(app, callQueueSlot(app, CALLER_CID));

        await activePeer.waitForMessage(endedOrCancelled(activeCallId));
        await caller.waitForMessage(joinedByApp(callerCallId));
        await waitForCallDisplayOf(app, CALLER_CID);

        await Promise.all([
            caller.expectNoMessage(endedOrCancelled(callerCallId), NO_REACTION_WINDOW_MS),
            expectNoModalOverlay(app, NO_REACTION_WINDOW_MS),
        ]);

        // The same late accept a caller cancelling during the ICE refresh produces.
        const lateAccept = await app.execute(async (callId: string) => {
            try {
                await window.__TAURI_INTERNALS__.invoke("signaling_accept_call", {callId});
                return null;
            } catch (e) {
                return JSON.stringify(e);
            }
        }, callerCallId);
        if (lateAccept !== null) {
            throw new Error(`A repeated accept failed: ${lateAccept}`);
        }

        await waitForCallDisplayOf(app, CALLER_CID);
        await waitForCallColor(app, callDisplaySlot(app), {active: true});
    });

    it("should tear down an established call between two clients when enabled", async () => {
        const app = getClient("clientA");
        const peerApp = getClient("clientB");
        await loginAndConnect(peerApp, ACTIVE_PEER_CID);
        const [caller] = await connectPeers(CALLER_CID);

        // A real peer negotiates media, so the swap has to close a live WebRTC connection.
        await startCallTo(peerApp, APP_CID);
        await callQueueSlot(app, ACTIVE_PEER_CID).waitForDisplayed();
        await click(app, callQueueSlot(app, ACTIVE_PEER_CID));
        await waitForConnectedCall(app);
        await waitForConnectedCall(peerApp);

        const callerCallId = await ringIn(app, caller);
        await click(app, callQueueSlot(app, CALLER_CID));

        await caller.waitForMessage(joinedByApp(callerCallId));
        await waitForCallDisplayOf(app, CALLER_CID);
        await callDisplaySlot(peerApp).waitForDisplayed({
            reverse: true,
            timeoutMsg: "The peer app kept a call display after its call was ended",
        });
        await Promise.all([
            expectNoModalOverlay(app, NO_REACTION_WINDOW_MS),
            expectNoModalOverlay(peerApp, NO_REACTION_WINDOW_MS),
        ]);
        if (await callDisplaySlot(peerApp).isExisting()) {
            throw new Error("The peer app shows a call display again after its call was ended");
        }
        await waitForCallDisplayOf(app, CALLER_CID);
    });

    it("should keep the active call and leave the incoming call ringing when disabled", async () => {
        const app = getClient("clientA");
        await disableEndActiveCallOnAccept(app);
        const [activePeer, caller] = await connectPeers(ACTIVE_PEER_CID, CALLER_CID);

        const activeCallId = await acceptCallFrom(app, activePeer);
        const callerCallId = await ringIn(app, caller);

        await click(app, callQueueSlot(app, CALLER_CID));
        await click(app, await showClientKey(app, CALLER_CID));

        // A client key that missed the incoming call would invite the caller instead.
        await Promise.all([
            activePeer.expectNoMessage(endedOrCancelled(activeCallId), NO_REACTION_WINDOW_MS),
            caller.expectNoMessage(
                msg => msg.type === "callInvitation" || joinedByApp(callerCallId)(msg),
                NO_REACTION_WINDOW_MS,
            ),
        ]);
        await waitForCallDisplayOf(app, ACTIVE_PEER_CID);
        await callQueueSlot(app, CALLER_CID).waitForDisplayed();
    });

    it("should not end an outgoing call that is still ringing when enabled", async () => {
        const app = getClient("clientA");
        const [target, caller] = await connectPeers(ACTIVE_PEER_CID, CALLER_CID);

        await startCallTo(app, ACTIVE_PEER_CID);
        const invitation = await target.waitForMessage(msg => msg.type === "callInvitation");
        await waitForOutgoingCall(app);

        const callerCallId = await ringIn(app, caller);

        await click(app, callQueueSlot(app, CALLER_CID));
        await click(app, await showClientKey(app, CALLER_CID));

        await Promise.all([
            target.expectNoMessage(
                endedOrCancelled(invitation.callId as string),
                NO_REACTION_WINDOW_MS,
            ),
            caller.expectNoMessage(
                msg => msg.type === "callInvitation" || joinedByApp(callerCallId)(msg),
                NO_REACTION_WINDOW_MS,
            ),
        ]);
        await waitForOutgoingCall(app);
        await waitForCallDisplayOf(app, ACTIVE_PEER_CID);
        await callQueueSlot(app, CALLER_CID).waitForDisplayed();
    });
});
