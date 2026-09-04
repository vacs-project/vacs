import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {
    callDisplaySlot,
    click,
    clientKey,
    conferenceKey,
    getClient,
    inviteTarget,
    showClientKey,
    waitForCallColor,
    waitForErroredKey,
} from "../helpers/browser.ts";
import {SignalingTestClient} from "../helpers/signaling-client.ts";

// Users without matching datafeed controllers: their sessions stay
// positionless (display name = CID) regardless of datafeed sync timing, so
// every client key keeps a stable label for the whole test.
const CID_A = "10000004";
const CID_B = "10000005";
const CID_C = "10000006";
// A fourth client for the size limit, present as a raw signaling client: the
// suite runs three app instances, and the refused target never needs a UI.
const CID_D = "10000007";

/**
 * Clicks another client's key. Depending on the current call state that
 * starts a call, accepts the call ringing from that client, drops it from the
 * conference (leader only) or ends the call; the tests say which one they
 * expect at each call site.
 */
async function clickClientKey(browser: WebdriverIO.Browser, displayName: string): Promise<void> {
    const key = await showClientKey(browser, displayName);
    await click(browser, key);
}

/**
 * Waits until the client shows all the given clients as joined participants
 * of its current call: a client key is steady green exactly while that client
 * is in the call display's joined participants.
 */
async function waitForJoined(browser: WebdriverIO.Browser, displayNames: string[]): Promise<void> {
    for (const displayName of displayNames) {
        await waitForCallColor(browser, await showClientKey(browser, displayName), {active: true});
    }
}

/**
 * Grows a fresh 1:1 call into a three-way conference led by A: A calls B, B
 * accepts, A adds C through the CONF key and C accepts. Returns once every
 * client sees the other two as joined participants, which is also the point
 * at which A knows it is the conference leader (the update that adds C
 * carries the leader).
 */
async function establishConference(): Promise<void> {
    const clientA = getClient("clientA");
    const clientB = getClient("clientB");
    const clientC = getClient("clientC");

    await clickClientKey(clientA, CID_B);
    await clickClientKey(clientB, CID_A);
    await waitForCallColor(clientA, clientKey(clientA, CID_B), {active: true});

    // The CONF key unlocks only once a call is established and its media is
    // connected; whoever presses it becomes the conference leader.
    const conf = conferenceKey(clientA);
    await clientA.waitUntil(async () => await conf.isEnabled(), {
        timeoutMsg: "CONF key did not unlock for the established call",
    });
    await click(clientA, conf);
    await clickClientKey(clientA, CID_C);

    await clickClientKey(clientC, CID_A);

    await waitForJoined(clientA, [CID_B, CID_C]);
    await waitForJoined(clientB, [CID_A, CID_C]);
    await waitForJoined(clientC, [CID_A, CID_B]);
}

describe("Conference Calls", () => {
    let peers: SignalingTestClient[] = [];

    beforeEach(async () => {
        await resetMockState();
        await restartApps();

        await loginAndConnect(getClient("clientA"), CID_A);
        await loginAndConnect(getClient("clientB"), CID_B);
        await loginAndConnect(getClient("clientC"), CID_C);
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

    it("should connect a conference whose callees were invited before either accepted", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        const clientC = getClient("clientC");

        await clickClientKey(clientA, CID_B);
        await callDisplaySlot(clientA).waitForDisplayed();
        // Second target while the first one is still ringing, so both callees
        // ring at once and the call is a conference before anyone accepts.
        await inviteTarget(clientA, CID_A, CID_C);

        // Both callees accept by pressing the caller's client key, which
        // avoids keying off the label of a ringing conference invitation.
        await clickClientKey(clientB, CID_A);
        await clickClientKey(clientC, CID_A);

        await waitForJoined(clientA, [CID_B, CID_C]);
        await waitForJoined(clientB, [CID_A, CID_C]);
        await waitForJoined(clientC, [CID_A, CID_B]);

        // Give the two peer connections every client now holds a moment to
        // negotiate, then verify none of them failed.
        await clientA.pause(1500);
        for (const client of [clientA, clientB, clientC]) {
            await client.$('img[alt="Disconnected"]').waitForDisplayed({reverse: true});
        }
        await waitForCallColor(clientA, clientKey(clientA, CID_C), {active: true});
    });

    it("should grow an established call into a conference when the leader invites a third client", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        const clientC = getClient("clientC");

        // The roster of all three clients is asserted by the waits inside.
        await establishConference();

        await clientA.pause(1500);
        for (const client of [clientA, clientB, clientC]) {
            await client.$('img[alt="Disconnected"]').waitForDisplayed({reverse: true});
        }
        await waitForCallColor(clientC, clientKey(clientC, CID_B), {active: true});
    });

    it("should continue the conference when a non-leader participant hangs up", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        const clientC = getClient("clientC");

        await establishConference();

        // B is a plain participant: ending its own call removes only B.
        await click(clientB, await clientB.$("button=END"));
        await callDisplaySlot(clientB).waitForDisplayed({reverse: true});

        await waitForCallColor(clientA, clientKey(clientA, CID_B), {active: false});
        await waitForCallColor(clientC, clientKey(clientC, CID_B), {active: false});
        await waitForCallColor(clientA, clientKey(clientA, CID_C), {active: true});
        await waitForCallColor(clientC, clientKey(clientC, CID_A), {active: true});
        await callDisplaySlot(clientA).waitForDisplayed();
        await callDisplaySlot(clientC).waitForDisplayed();
    });

    it("should end the call for everyone when the conference leader hangs up", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        const clientC = getClient("clientC");

        await establishConference();

        await click(clientA, await clientA.$("button=END"));

        await callDisplaySlot(clientA).waitForDisplayed({reverse: true});
        await callDisplaySlot(clientB).waitForDisplayed({reverse: true});
        await callDisplaySlot(clientC).waitForDisplayed({reverse: true});
        await waitForCallColor(clientB, clientKey(clientB, CID_C), {active: false});
        await waitForCallColor(clientC, clientKey(clientC, CID_B), {active: false});
    });

    it("should let only the conference leader drop a participant", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        const clientC = getClient("clientC");

        await establishConference();

        // Conference control is the leader's: B holds no affordance to invite
        // or drop anyone, its CONF key stays locked.
        const confB = conferenceKey(clientB);
        await clientB.waitUntil(async () => !(await confB.isEnabled()), {
            timeoutMsg: "CONF key did not lock for a non-leader participant",
        });
        if (!(await conferenceKey(clientA).isEnabled())) {
            throw new Error("CONF key is locked for the conference leader");
        }

        // The leader drops C by pressing its client key; A and B stay in the
        // call. The next test covers the same click on a non-leader, which
        // hangs that client up instead of dropping anyone.
        await clickClientKey(clientA, CID_C);

        await callDisplaySlot(clientC).waitForDisplayed({reverse: true});
        await waitForCallColor(clientA, clientKey(clientA, CID_C), {active: false});
        await waitForCallColor(clientB, clientKey(clientB, CID_C), {active: false});
        await waitForCallColor(clientA, clientKey(clientA, CID_B), {active: true});
        await waitForCallColor(clientB, clientKey(clientB, CID_A), {active: true});
        await callDisplaySlot(clientA).waitForDisplayed();
        await callDisplaySlot(clientB).waitForDisplayed();
    });

    it("should hang up a non-leader that presses another participant's key", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        const clientC = getClient("clientC");

        await establishConference();

        // Decided behavior: dropping a participant is the leader's alone, so
        // the same key press falls through to ending B's own call rather than
        // removing C.
        await clickClientKey(clientB, CID_C);

        // B is idle again: no call display, and neither remaining participant
        // is shown as in a call with it.
        await callDisplaySlot(clientB).waitForDisplayed({reverse: true});
        await waitForCallColor(clientB, clientKey(clientB, CID_A), {active: false});
        await waitForCallColor(clientB, clientKey(clientB, CID_C), {active: false});

        // A and C keep the call between them.
        await waitForCallColor(clientA, clientKey(clientA, CID_B), {active: false});
        await waitForCallColor(clientC, clientKey(clientC, CID_B), {active: false});
        await waitForCallColor(clientA, clientKey(clientA, CID_C), {active: true});
        await waitForCallColor(clientC, clientKey(clientC, CID_A), {active: true});
        await callDisplaySlot(clientA).waitForDisplayed();
        await callDisplaySlot(clientC).waitForDisplayed();
    });

    it("should end the conference for everyone when the leader disconnects", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        const clientC = getClient("clientC");

        await establishConference();

        // The leader vanishes instead of hanging up: signaling_disconnect
        // drops the websocket and tears the call down locally without ever
        // sending a call end, which is the closest reachable stand-in for a
        // crashed or network-dropped leader. Ending the conference for the
        // survivors is then the server's job alone.
        await clientA.execute(async () => {
            await window.__TAURI_INTERNALS__.invoke("signaling_disconnect");
        });

        // The leader itself is back on the connect page.
        await clientA.$("button*=Connect").waitForDisplayed();

        // Both survivors lose the call, not just the leader's leg of it.
        await callDisplaySlot(clientB).waitForDisplayed({reverse: true});
        await callDisplaySlot(clientC).waitForDisplayed({reverse: true});
        await waitForCallColor(clientB, clientKey(clientB, CID_C), {active: false});
        await waitForCallColor(clientC, clientKey(clientC, CID_B), {active: false});
        // The leader also left the client list, so its key is gone entirely.
        await clientKey(clientB, CID_A).waitForDisplayed({reverse: true});
        await clientKey(clientC, CID_A).waitForDisplayed({reverse: true});
    });

    it("should refuse an invite beyond the max conference size", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        const clientC = getClient("clientC");
        // Connected before the conference forms, so its client key is in the
        // list by the time the refused invite has to annotate it.
        const peerD = await connectPeer(CID_D);

        await establishConference();

        // The frontend carries its own guard against exceeding maxConfSize
        // (from the session info) and would refuse this before it ever
        // reached the server, so the invite goes out through the command a
        // client key would invoke. What is under test is the server's
        // refusal and how the client renders it.
        await inviteTarget(clientA, CID_A, CID_D);

        // The refusal annotates the target that could not join and names the
        // reason in the info grid. The cell's title carries the raw reason,
        // which the client prefixes with where the error came from and the
        // CSS then uppercases.
        await waitForErroredKey(clientA, await showClientKey(clientA, CID_D));
        await clientA.$('//div[@title="Remote Max conf size"]').waitForDisplayed();

        // The fourth client never rang.
        await peerD.expectNoMessage(msg => msg.type === "callInvitation");

        // The running conference is untouched on all three clients.
        await waitForJoined(clientA, [CID_B, CID_C]);
        await waitForJoined(clientB, [CID_A, CID_C]);
        await waitForJoined(clientC, [CID_A, CID_B]);
        await callDisplaySlot(clientA).waitForDisplayed();
    });
});
