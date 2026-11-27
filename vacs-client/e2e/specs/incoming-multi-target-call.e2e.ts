import {appLogLinesSince, restartApps} from "../helpers/app-control.ts";
import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {
    callDisplaySlot,
    callQueueSlot,
    click,
    clientKey,
    conferenceAnswerKey,
    conferenceKey,
    expectNoModalOverlay,
    getClient,
    showClientKey,
    waitForCallColor,
    waitForCallDisplayOf,
    waitForCallingKey,
    waitForConferenceCallDisplay,
    waitForIdleKey,
    waitForPendingKey,
} from "../helpers/browser.ts";
import {type ServerMessage, SignalingTestClient} from "../helpers/signaling-client.ts";

// All positionless, so every key keeps its CID as label. The app has the
// lowest CID: the lower side of each pair sends the WebRTC offer, so the app
// offers on every link instead of waiting for one a raw client never sends.
const APP_CID = "10000004";
const CALLER_CID = "10000005";
const CO_TARGET_CID = "10000006";

const callUpdate =
    (callId: string, check: (msg: ServerMessage) => boolean) => (msg: ServerMessage) =>
        msg.type === "callUpdate" && msg.callId === callId && check(msg);

const joined = (msg: ServerMessage, cid: string) => msg.joinedParticipants?.[cid] !== undefined;

const offerFromApp = (callId: string) => (msg: ServerMessage) =>
    msg.type === "webrtcOffer" && msg.callId === callId && msg.fromClientId === APP_CID;

describe("Incoming Multi-Target Call", () => {
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

    function inviteAppAndCoTarget(caller: SignalingTestClient): string {
        return caller.inviteTargets([{client: APP_CID}, {client: CO_TARGET_CID}]);
    }

    async function acceptOnApp(
        app: WebdriverIO.Browser,
        caller: SignalingTestClient,
        callId: string,
    ): Promise<void> {
        await conferenceAnswerKey(app).waitForDisplayed();
        await click(app, conferenceAnswerKey(app));
        await caller.waitForMessage(callUpdate(callId, msg => joined(msg, APP_CID)));
    }

    async function waitForThreeParties(app: WebdriverIO.Browser): Promise<void> {
        await waitForConferenceCallDisplay(app);
        await waitForCallColor(app, await showClientKey(app, CALLER_CID), {active: true});
        await waitForCallColor(app, await showClientKey(app, CO_TARGET_CID), {active: true});
    }

    it("should ring as a conference invitation without lighting up the co-target", async () => {
        const app = getClient("clientA");
        const [caller, coTarget] = await connectPeers(CALLER_CID, CO_TARGET_CID);

        const callId = inviteAppAndCoTarget(caller);
        await coTarget.waitForMessage(
            msg => msg.type === "callInvitation" && msg.callId === callId,
        );

        await conferenceAnswerKey(app).waitForDisplayed();
        if (await callQueueSlot(app, CALLER_CID).isExisting()) {
            throw new Error("An answer key names the caller of a multi-target invitation");
        }
        if (await callDisplaySlot(app).isExisting()) {
            throw new Error("A ringing invitation produced a call display");
        }

        await waitForCallingKey(app, await showClientKey(app, CALLER_CID));
        await waitForIdleKey(app, clientKey(app, CO_TARGET_CID));
    });

    it("should join a conference led by the caller when the app accepts before the co-target", async () => {
        const app = getClient("clientA");
        const [caller, coTarget] = await connectPeers(CALLER_CID, CO_TARGET_CID);

        const callId = inviteAppAndCoTarget(caller);
        await acceptOnApp(app, caller, callId);

        await waitForConferenceCallDisplay(app);
        await waitForCallColor(app, await showClientKey(app, CALLER_CID), {active: true});
        await waitForPendingKey(app, await showClientKey(app, CO_TARGET_CID));
        await caller.waitForMessage(offerFromApp(callId));

        coTarget.accept(callId);
        await coTarget.waitForMessage(
            callUpdate(callId, msg => joined(msg, APP_CID) && msg.conferenceLeader === CALLER_CID),
        );

        await waitForThreeParties(app);
        await coTarget.waitForMessage(offerFromApp(callId));

        // A leader's press would drop the co-target; a non-leader's hangs
        // itself up.
        await click(app, await showClientKey(app, CO_TARGET_CID));

        await callDisplaySlot(app).waitForDisplayed({reverse: true});
        await caller.waitForMessage(
            callUpdate(callId, msg => !joined(msg, APP_CID) && joined(msg, CO_TARGET_CID)),
        );
        await coTarget.expectNoMessage(msg => msg.type === "callEnd", 1000);
        await waitForCallColor(app, clientKey(app, CALLER_CID), {active: false});
        await waitForCallColor(app, clientKey(app, CO_TARGET_CID), {active: false});
    });

    it("should lock the CONF key for both receivers and end with the caller as leader", async () => {
        const app = getClient("clientA");
        // A real co-target: the CONF key also stays locked until a peer link
        // carries media, and only two apps can negotiate one.
        const coApp = getClient("clientB");
        await loginAndConnect(coApp, CO_TARGET_CID);
        const [caller] = await connectPeers(CALLER_CID);
        const since = new Date();

        const callId = inviteAppAndCoTarget(caller);
        await acceptOnApp(app, caller, callId);

        await conferenceAnswerKey(coApp).waitForDisplayed();
        await click(coApp, conferenceAnswerKey(coApp));
        await caller.waitForMessage(
            callUpdate(
                callId,
                msg => joined(msg, CO_TARGET_CID) && msg.conferenceLeader === CALLER_CID,
            ),
        );

        await waitForThreeParties(app);
        await waitForConferenceCallDisplay(coApp);
        await waitForCallColor(coApp, await showClientKey(coApp, CALLER_CID), {active: true});
        await waitForCallColor(coApp, await showClientKey(coApp, APP_CID), {active: true});

        // Both instances share one log directory; the peer id tells the ends apart.
        await app.waitUntil(
            async () =>
                appLogLinesSince(since, `Connected to peer ${CO_TARGET_CID} in call ${callId}`)
                    .length > 0 &&
                appLogLinesSince(since, `Connected to peer ${APP_CID} in call ${callId}`).length >
                    0,
            {timeoutMsg: "The link between the two apps did not connect"},
        );

        // With media up only leadership keeps the key locked; the window lets
        // the connected state reach both frontends first.
        const start = Date.now();
        while (Date.now() - start < 1500) {
            for (const [name, browser] of [
                ["app", app],
                ["co-target app", coApp],
            ] as const) {
                if (await conferenceKey(browser).isEnabled()) {
                    throw new Error(`The CONF key unlocked for the ${name}, a non-leader`);
                }
            }
            await app.pause(250);
        }

        // Only the leader leaving ends the call for everyone; any other
        // leader would have left the two apps talking.
        caller.end(callId);
        await callDisplaySlot(app).waitForDisplayed({reverse: true});
        await callDisplaySlot(coApp).waitForDisplayed({reverse: true});
    });

    it("should join the established call when the co-target accepts before the app", async () => {
        const app = getClient("clientA");
        const [caller, coTarget] = await connectPeers(CALLER_CID, CO_TARGET_CID);

        const callId = inviteAppAndCoTarget(caller);
        // The accept arrives on another socket and could beat the invite to the server.
        await coTarget.waitForMessage(
            msg => msg.type === "callInvitation" && msg.callId === callId,
        );
        coTarget.accept(callId);
        await caller.waitForMessage(callUpdate(callId, msg => joined(msg, CO_TARGET_CID)));

        await conferenceAnswerKey(app).waitForDisplayed();
        await waitForCallingKey(app, await showClientKey(app, CO_TARGET_CID));

        await click(app, conferenceAnswerKey(app));
        await coTarget.waitForMessage(
            callUpdate(callId, msg => joined(msg, APP_CID) && msg.conferenceLeader === CALLER_CID),
        );

        await waitForThreeParties(app);
        await caller.waitForMessage(offerFromApp(callId));
        await coTarget.waitForMessage(offerFromApp(callId));

        await click(app, await showClientKey(app, CALLER_CID));

        await callDisplaySlot(app).waitForDisplayed({reverse: true});
        await coTarget.waitForMessage(
            callUpdate(callId, msg => !joined(msg, APP_CID) && joined(msg, CALLER_CID)),
        );
    });

    it("should keep a plain call with the caller when the co-target rejects", async () => {
        const app = getClient("clientA");
        const [caller, coTarget] = await connectPeers(CALLER_CID, CO_TARGET_CID);

        const callId = inviteAppAndCoTarget(caller);
        await acceptOnApp(app, caller, callId);
        await waitForPendingKey(app, await showClientKey(app, CO_TARGET_CID));

        coTarget.reject(callId);

        await waitForCallDisplayOf(app, CALLER_CID);
        await waitForCallColor(app, callDisplaySlot(app), {active: true});
        await waitForIdleKey(app, clientKey(app, CO_TARGET_CID));
        await waitForCallColor(app, clientKey(app, CALLER_CID), {active: true});
        await expectNoModalOverlay(app, 1000);
    });

    it("should ring and connect as a plain call when the caller cancels the co-target", async () => {
        const app = getClient("clientA");
        const [caller, coTarget] = await connectPeers(CALLER_CID, CO_TARGET_CID);

        const callId = inviteAppAndCoTarget(caller);
        await conferenceAnswerKey(app).waitForDisplayed();

        caller.dropTarget(callId, {client: CO_TARGET_CID});
        await coTarget.waitForMessage(msg => msg.type === "callCancelled" && msg.callId === callId);

        await callQueueSlot(app, CALLER_CID).waitForDisplayed();
        await conferenceAnswerKey(app).waitForDisplayed({reverse: true});

        await click(app, callQueueSlot(app, CALLER_CID));
        await caller.waitForMessage(callUpdate(callId, msg => joined(msg, APP_CID)));

        await waitForCallDisplayOf(app, CALLER_CID);
        await waitForCallColor(app, callDisplaySlot(app), {active: true});
        await waitForIdleKey(app, await showClientKey(app, CO_TARGET_CID));
    });

    it("should clear the ringing invitation when the caller ends the call", async () => {
        const app = getClient("clientA");
        const [caller, coTarget] = await connectPeers(CALLER_CID, CO_TARGET_CID);

        const callId = inviteAppAndCoTarget(caller);
        await conferenceAnswerKey(app).waitForDisplayed();
        await waitForCallingKey(app, await showClientKey(app, CALLER_CID));

        caller.end(callId);
        await coTarget.waitForMessage(msg => msg.type === "callCancelled" && msg.callId === callId);

        await conferenceAnswerKey(app).waitForDisplayed({reverse: true});
        await waitForIdleKey(app, clientKey(app, CALLER_CID));
        if (await callDisplaySlot(app).isExisting()) {
            throw new Error("The cancelled invitation left a call display behind");
        }
        await expectNoModalOverlay(app, 1000);
    });
});
