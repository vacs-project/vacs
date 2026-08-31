import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {
    callDisplaySlot,
    click,
    clientKey,
    conferenceKey,
    getClient,
    waitForCallColor,
} from "../helpers/browser.ts";

// Users without matching datafeed controllers: their sessions stay
// positionless (display name = CID) regardless of datafeed sync timing, so
// every client key keeps a stable label for the whole test.
const CID_A = "10000004";
const CID_B = "10000005";
const CID_C = "10000006";

/**
 * Brings the client key for the given display name into view, opening the
 * "OTHER" client group (all clients without a resolved VATSIM position) when
 * the page still shows the group keys. Unlike the one-shot helper in
 * specs/call.e2e.ts this is safe to call repeatedly: the group key is gone
 * once the group is open, and END resets the page back to the group keys.
 */
async function showClientKey(
    browser: WebdriverIO.Browser,
    displayName: string,
): Promise<ChainablePromiseElement> {
    await browser.waitUntil(
        async () => {
            if (await clientKey(browser, displayName).isExisting()) return true;
            const group = await browser.$("button*=OTHER");
            if (await group.isDisplayed()) await click(browser, group);
            return false;
        },
        {timeoutMsg: `Client key for ${displayName} did not appear in the OTHER group`},
    );
    const key = clientKey(browser, displayName);
    await key.waitForDisplayed();
    return key;
}

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
 * Invites another target into the client's current call through the same
 * signaling command a client key invokes. Needed only where the UI offers no
 * affordance: a fresh call cannot be given a second target, because the CONF
 * key stays locked until a call is established.
 */
async function inviteTarget(
    browser: WebdriverIO.Browser,
    ownCid: string,
    targetCid: string,
): Promise<void> {
    const result = await browser.execute(
        async (own: string, target: string) => {
            try {
                await window.__TAURI_INTERNALS__.invoke("signaling_invite_to_call", {
                    source: {clientId: own},
                    targets: [{client: target}],
                    prio: false,
                });
                return {ok: true as const};
            } catch (e) {
                return {ok: false as const, error: String(e)};
            }
        },
        ownCid,
        targetCid,
    );

    if (!result.ok) {
        throw new Error(`signaling_invite_to_call failed for ${targetCid}: ${result.error}`);
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
    beforeEach(async () => {
        await resetMockState();
        await restartApps();

        await loginAndConnect(getClient("clientA"), CID_A);
        await loginAndConnect(getClient("clientB"), CID_B);
        await loginAndConnect(getClient("clientC"), CID_C);
    });

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
});
