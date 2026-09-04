import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {
    callDisplaySlot,
    callQueueSlot,
    click,
    getClient,
    startCallTo,
    waitForOutgoingCall,
    waitForRejectedCall,
} from "../helpers/browser.ts";
import {closeRemoteBrowser, openRemoteBrowser, setRemoteEnabled} from "../helpers/remote.ts";
import {SignalingTestClient} from "../helpers/signaling-client.ts";

const APP_CID = "10000004";
// A raw signaling client without a datafeed controller keeps its CID as
// display name, which is also the label of its client key.
const PEER_CID = "10000005";

describe("Remote Call State", () => {
    let peers: SignalingTestClient[] = [];

    beforeEach(async () => {
        // Before anything else: a page left on the remote frontend keeps
        // reconnecting, and would hydrate itself into the app this test is
        // about to start, defeating every "connects mid-call" assertion.
        await closeRemoteBrowser();

        await resetMockState();
        await restartApps();

        const clientA = getClient("clientA");
        await loginAndConnect(clientA, APP_CID);
        await setRemoteEnabled(clientA, true);
    });

    afterEach(() => {
        for (const peer of peers) {
            peer.disconnect();
        }
        peers = [];
    });

    async function connectPeer(cid: string): Promise<SignalingTestClient> {
        const peer = await SignalingTestClient.connect(cid);
        peers.push(peer);
        return peer;
    }

    it("should show a ringing outgoing call to a browser that connects mid-call", async () => {
        const clientA = getClient("clientA");
        const peer = await connectPeer(PEER_CID);

        await startCallTo(clientA, PEER_CID);
        await peer.waitForMessage(msg => msg.type === "callInvitation");
        await waitForOutgoingCall(clientA);

        // The session snapshot carries no call state on purpose; the display
        // can only come from the store sync requested after hydrating.
        const remoteBrowser = await openRemoteBrowser();
        await waitForOutgoingCall(remoteBrowser);
    });

    it("should show a queued incoming call to a browser that connects mid-call", async () => {
        const clientA = getClient("clientA");
        const peer = await connectPeer(PEER_CID);

        peer.invite(APP_CID);
        await callQueueSlot(clientA, PEER_CID).waitForDisplayed();

        // Live syncs carry a null incomingCalls, so a browser that missed the
        // invitation event has only the bootstrap sync to learn about it.
        const remoteBrowser = await openRemoteBrowser();
        await callQueueSlot(remoteBrowser, PEER_CID).waitForDisplayed();
    });

    it("should show a call placed from the browser as outgoing on both UIs", async () => {
        const clientA = getClient("clientA");
        // Connected before the browser hydrates, so its client key is part of
        // the snapshot's client list and can be pressed right away.
        const peer = await connectPeer(PEER_CID);
        const remoteBrowser = await openRemoteBrowser();

        await startCallTo(remoteBrowser, PEER_CID);
        await peer.waitForMessage(msg => msg.type === "callInvitation");

        // Both UIs build the display from the same backend event, not from
        // the reply to the invite the browser sent.
        await waitForOutgoingCall(remoteBrowser);
        await waitForOutgoingCall(clientA);
    });

    it("should converge on the rejected display of a browser call and on its dismissal", async () => {
        const clientA = getClient("clientA");
        const peer = await connectPeer(PEER_CID);
        // Rejecting from inside the receive path puts the cancellation ahead
        // of the reply to the browser's invite.
        peer.autoRejectInvitations();
        const remoteBrowser = await openRemoteBrowser();

        await startCallTo(remoteBrowser, PEER_CID);

        await waitForRejectedCall(remoteBrowser);
        await waitForRejectedCall(clientA);

        // Dismissing a terminal display is store-local wherever it happens;
        // the other UI follows through the call store sync.
        await click(clientA, callDisplaySlot(clientA));
        await callDisplaySlot(clientA).waitForDisplayed({reverse: true});
        await callDisplaySlot(remoteBrowser).waitForDisplayed({reverse: true});
    });
});
