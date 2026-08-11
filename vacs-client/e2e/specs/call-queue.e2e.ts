import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnect, removeController, resetMockState} from "../helpers/auth.ts";
import {callQueueSlot, click, getClient} from "../helpers/browser.ts";
import {SignalingTestClient} from "../helpers/signaling-client.ts";

const APP_CID = "10000004";
// Additional callers connected via the programmatic signaling client.
const CALLER_CIDS = ["10000001", "10000002", "10000003", "10000005", "10000006", "10000007"];

const PRIO_YELLOW = "bg-[#f8ec2c]";

describe("Call Queue", () => {
    let callers: SignalingTestClient[] = [];

    beforeEach(async () => {
        await resetMockState();
        // The seed lists 10000001-3 as online controllers; once their grace
        // period expires the server assigns them positions and the client
        // keys relabel from CID to position, breaking the CID selectors.
        // The callers here must stay positionless.
        for (const cid of ["10000001", "10000002", "10000003"]) {
            await removeController(cid);
        }
        await restartApps();

        await loginAndConnect(getClient("clientA"), APP_CID);
    });

    afterEach(() => {
        for (const caller of callers) {
            caller.disconnect();
        }
        callers = [];
    });

    async function connectCallers(count: number): Promise<SignalingTestClient[]> {
        const connected = await Promise.all(
            CALLER_CIDS.slice(0, count).map(cid => SignalingTestClient.connect(cid)),
        );
        callers.push(...connected);
        return connected;
    }

    it("should queue multiple incoming calls and accept them in turn", async () => {
        const clientA = getClient("clientA");
        const [caller1, caller2] = await connectCallers(2);

        const callId1 = caller1.invite(APP_CID);
        const callId2 = caller2.invite(APP_CID);

        // Accepting a call re-renders the queue and can replace the queued
        // slot's DOM node, so a held element handle would go stale; query
        // the second slot fresh at each use.
        const answerKey2 = () => callQueueSlot(clientA, caller2.cid);

        // Both calls appear as answer keys.
        const answerKey1 = callQueueSlot(clientA, caller1.cid);
        await answerKey1.waitForDisplayed();
        await answerKey2().waitForDisplayed();

        // Accepting the first call leaves the second queued.
        await click(clientA, answerKey1);
        await caller1.waitForMessage(msg => msg.type === "callAccept" && msg.callId === callId1);
        await answerKey2().waitForDisplayed();

        // While busy, clicking the queued call must not accept it.
        await click(clientA, await answerKey2());
        await clientA.pause(500);
        await answerKey2().waitForDisplayed();

        // After ending the first call, the second can be accepted.
        const endButton = await clientA.$("button=END");
        await click(clientA, endButton);
        await caller1.waitForMessage(msg => msg.type === "callEnd" && msg.callId === callId1);

        await click(clientA, await answerKey2());
        await caller2.waitForMessage(msg => msg.type === "callAccept" && msg.callId === callId2);
        await click(clientA, endButton);
        await caller2.waitForMessage(msg => msg.type === "callEnd" && msg.callId === callId2);
    });

    it("should reject incoming calls beyond the queue limit", async () => {
        const clientA = getClient("clientA");
        const connected = await connectCallers(6);
        const queued = connected.slice(0, 5);
        const rejected = connected[5];

        for (const caller of queued) {
            caller.invite(APP_CID);
        }
        for (const caller of queued) {
            await callQueueSlot(clientA, caller.cid).waitForDisplayed();
        }

        // The sixth call exceeds the queue limit and is rejected as busy.
        const rejectedCallId = rejected.invite(APP_CID);
        const cancellation = await rejected.waitForMessage(
            msg => msg.type === "callCancelled" && msg.callId === rejectedCallId,
        );
        if (!JSON.stringify(cancellation.reason).includes("ejected")) {
            throw new Error(
                `Expected rejection, got cancellation reason ${JSON.stringify(cancellation.reason)}`,
            );
        }
        await callQueueSlot(clientA, rejected.cid).waitForDisplayed({reverse: true});
    });

    it("should display and connect priority calls", async () => {
        const clientA = getClient("clientA");
        const [caller] = await connectCallers(1);

        const callId = caller.invite(APP_CID, {prio: true});

        // The answer key blinks yellow for priority calls.
        const answerKey = callQueueSlot(clientA, caller.cid);
        await answerKey.waitForDisplayed();
        await clientA.waitUntil(
            async () => {
                const classes = (await answerKey.getAttribute("class")) ?? "";
                return classes.includes(PRIO_YELLOW);
            },
            {interval: 150, timeoutMsg: "Priority call did not blink yellow"},
        );

        // Accepted priority calls show steady yellow.
        await click(clientA, answerKey);
        await caller.waitForMessage(msg => msg.type === "callAccept" && msg.callId === callId);
        await clientA.waitUntil(
            async () => {
                const classes =
                    (await callQueueSlot(clientA, caller.cid).getAttribute("class")) ?? "";
                return classes.includes(PRIO_YELLOW);
            },
            {timeoutMsg: "Accepted priority call is not shown in yellow"},
        );

        const endButton = await clientA.$("button=END");
        await click(clientA, endButton);
        await caller.waitForMessage(msg => msg.type === "callEnd" && msg.callId === callId);
    });
});
