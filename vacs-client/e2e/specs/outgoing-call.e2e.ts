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
import {SignalingTestClient} from "../helpers/signaling-client.ts";

const APP_CID = "10000004";
// Raw signaling clients: positionless CIDs, so their client keys keep the CID
// as label and no controller has to be removed from the mock datafeed.
const TARGET_CID = "10000005";
const OTHER_CID = "10000006";

// The client's default unanswered-call timeout. Not configurable in E2E
// builds (the e2e feature skips config files and the nested key is out of
// reach of the env-var layer), so the auto-hangup test really does wait.
const AUTO_HANGUP_MS = 60_000;

describe("Outgoing Call Display", () => {
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

    async function connectPeer(cid: string): Promise<SignalingTestClient> {
        const peer = await SignalingTestClient.connect(cid);
        peers.push(peer);
        return peer;
    }

    it("should show the rejection of a call that is refused before the invite returns", async () => {
        const clientA = getClient("clientA");
        const target = await connectPeer(TARGET_CID);
        // Rejecting from inside the receive path is what makes the server's
        // cancellation reach the app ahead of the invite command's own reply.
        const stopAutoReject = target.autoRejectInvitations();

        await startCallTo(clientA, TARGET_CID);

        await waitForRejectedCall(clientA);

        // The losing reply arrives within milliseconds; a display rebuilt from
        // it would have replaced the rejection with a bare outgoing call long
        // before this second check.
        await clientA.pause(1500);
        await waitForRejectedCall(clientA);

        // A terminal display is dismissed by clicking it.
        await click(clientA, callDisplaySlot(clientA));
        await callDisplaySlot(clientA).waitForDisplayed({reverse: true});

        // Redialing after the dismissal is the call that used to come up
        // without an outgoing display at all.
        stopAutoReject();
        await startCallTo(clientA, TARGET_CID);
        await waitForOutgoingCall(clientA);

        const invitations = await target.waitForMessages(
            msg => msg.type === "callInvitation",
            2,
            10_000,
        );
        if (invitations[0].callId === invitations[1].callId) {
            throw new Error("Redial reused the rejected call id instead of placing a new call");
        }
    });

    // Own suite purely for the raised timeout: this test waits out the full
    // unanswered-call timeout, which does not fit the 60s default. A per-test
    // this.timeout() cannot raise it, because WebdriverIO arms its own timer
    // from the runnable before the test body ever runs.
    describe("after the unanswered call timeout", function () {
        this.timeout(AUTO_HANGUP_MS + 30_000);

        it("should keep the timer when an unrelated incoming call ends", async () => {
            const clientA = getClient("clientA");
            const target = await connectPeer(TARGET_CID);
            const interrupter = await connectPeer(OTHER_CID);

            await startCallTo(clientA, TARGET_CID);
            const invitation = await target.waitForMessage(msg => msg.type === "callInvitation");
            await waitForOutgoingCall(clientA);

            // A second, unrelated call rings in and ends while the outgoing one is
            // still waiting for an answer.
            const interruptingCallId = interrupter.invite(APP_CID);
            const answerKey = callQueueSlot(clientA, OTHER_CID);
            await answerKey.waitForDisplayed();
            interrupter.end(interruptingCallId);
            await answerKey.waitForDisplayed({reverse: true});

            // Its teardown must leave the outgoing call alone.
            await waitForOutgoingCall(clientA);

            // The auto-hangup that survived it drops the target on the wire ...
            const cancelled = await target.waitForMessage(
                msg => msg.type === "callCancelled" && msg.callId === invitation.callId,
                AUTO_HANGUP_MS + 10_000,
            );
            if (JSON.stringify(cancelled.reason) !== JSON.stringify({errored: "autoHangup"})) {
                throw new Error(
                    `Expected an auto hangup cancellation, got ${JSON.stringify(cancelled.reason)}`,
                );
            }

            // ... and annotates the call display with the reason.
            const annotation = clientA.$('//div[@title="Remote Target did not answer"]');
            await annotation.waitForDisplayed();
        });
    });
});
