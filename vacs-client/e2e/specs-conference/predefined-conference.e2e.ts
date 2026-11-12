import path from "node:path";
import {fileURLToPath} from "node:url";
import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnectAs, removeController, resetMockState} from "../helpers/auth.ts";
import {
    answerKey,
    callDisplaySlot,
    callErrorCell,
    click,
    conferenceAnswerKey,
    conferenceKey,
    directAccessKey,
    expectNoModalOverlay,
    getClient,
    waitForCallColor,
    waitForConferenceCallDisplay,
    waitForIdleKey,
    waitForPendingKey,
} from "../helpers/browser.ts";
import {type ServerMessage, SignalingTestClient} from "../helpers/signaling-client.ts";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

// The station list still comes from the server's LOVV profile, so the fixture
// may only name stations that profile lists.
const PROFILE = path.resolve(__dirname, "..", "fixtures", "profile-conference.json");

// Removed from the datafeed for every test: a datafeed-only controller makes
// stations online that no client can answer, and LOVV_E_CTR would cover N1 and
// S1 through their parents.
const DATAFEED_CIDS = ["10000001", "10000002", "10000003"];

// CIDs without datafeed controllers keep the position they log in on. Each
// position covers exactly one station of the fixture: LOVV_FMP covers only
// LOVV_FMP (and gives the caller the LOVV station list and a call source),
// LOVV_N1_CTR only LOVV_N1, LOVV_S1_CTR only LOVV_S1.
const CALLER = {cid: "10000004", position: "LOVV_FMP"};
const N1 = {cid: "10000005", position: "LOVV_N1_CTR"};
const S1 = {cid: "10000006", position: "LOVV_S1_CTR"};
// A raw client for the cases that need to count invitations or cover what no
// app instance does. LOVV_S_CTR covers LOVV_S2 and, with LOVV_S1_CTR offline,
// LOVV_S1 through its parent.
const PEER_CID = "10000007";

// LOVV_FMP (the caller's own), LOVV_N1, LOVV_S1 and LOVV_E1 (covered by nobody).
const NSE_KEY = ["CONF", "NSE"];
const S12_KEY = ["CONF", "S12"];
const OFFLINE_KEY = ["CONF", "EW"];
const N1_STATION_KEY = ["STN", "N1"];
const S2_STATION_KEY = ["STN", "S2"];

async function loadTestProfile(browser: WebdriverIO.Browser): Promise<void> {
    const result = await browser.execute(async (profilePath: string) => {
        try {
            await window.__TAURI_INTERNALS__.invoke("app_load_test_profile", {path: profilePath});
            return {ok: true as const};
        } catch (e) {
            return {ok: false as const, error: String(e)};
        }
    }, PROFILE);

    if (!result.ok) {
        throw new Error(`app_load_test_profile failed: ${result.error}`);
    }
}

/** Loaded after connecting, so the session info's profile cannot replace the fixture. */
async function connect(
    browser: WebdriverIO.Browser,
    user: {cid: string; position: string},
): Promise<void> {
    await loginAndConnectAs(browser, user.cid, user.position);
    await loadTestProfile(browser);
    await directAccessKey(browser, ...NSE_KEY).waitForDisplayed();
}

async function waitForEnabled(
    browser: WebdriverIO.Browser,
    label: string[],
    enabled: boolean,
): Promise<void> {
    const key = directAccessKey(browser, ...label);
    await browser.waitUntil(async () => (await key.isEnabled()) === enabled, {
        timeoutMsg: `Key ${label.join(" ")} did not become ${enabled ? "enabled" : "disabled"}`,
    });
}

/**
 * Asserts over the window that the client shows no call error: neither the
 * info grid reason a TargetsNotFound or AlreadyParticipant answer would fill
 * in, nor an error overlay.
 */
async function expectNoCallError(browser: WebdriverIO.Browser, windowMs: number): Promise<void> {
    const start = Date.now();
    while (Date.now() - start < windowMs) {
        const reason = (await callErrorCell(browser).getText()).trim();
        if (reason !== "") {
            throw new Error(`The caller shows a call error: ${reason}`);
        }
        await expectNoModalOverlay(browser, 100);
    }
}

/** Asserts that no call rings in on the client for the whole window. */
async function expectNoIncomingCall(browser: WebdriverIO.Browser, windowMs: number): Promise<void> {
    const start = Date.now();
    while (Date.now() - start < windowMs) {
        if (await answerKey(browser).isExisting()) {
            throw new Error("A call rang in although the key press should have done nothing");
        }
        await new Promise(resolve => setTimeout(resolve, 100));
    }
}

/** Asserts that every client keeps a conference call display for the whole window. */
async function expectConferenceKept(
    clients: Record<string, WebdriverIO.Browser>,
    windowMs: number,
): Promise<void> {
    const start = Date.now();
    while (Date.now() - start < windowMs) {
        for (const [name, browser] of Object.entries(clients)) {
            const display = callDisplaySlot(browser);
            if (!(await display.isExisting()) || (await display.getText()).trim() !== "CONF") {
                throw new Error(`${name} left the conference after a conference key press`);
            }
        }
        await new Promise(resolve => setTimeout(resolve, 250));
    }
}

/**
 * Asserts over the window that a conference key keeps its default color: gray
 * without an inner highlight. Sampled faster than the 500ms blink, so neither
 * a ringing, joined, rejected nor errored look can slip through.
 */
async function expectPlainKey(
    browser: WebdriverIO.Browser,
    label: string[],
    windowMs: number,
): Promise<void> {
    const key = directAccessKey(browser, ...label);
    const start = Date.now();
    while (Date.now() - start < windowMs) {
        const classes = (await key.getAttribute("class")) ?? "";
        if (!classes.includes("bg-gray-300") || (await key.$("./div").isExisting())) {
            throw new Error(`Key ${label.join(" ")} showed call state: ${classes}`);
        }
        await new Promise(resolve => setTimeout(resolve, 150));
    }
}

/**
 * The CONF function key unlocks for the leader once its media is connected and
 * stays locked for everyone else, which makes it the visible leadership marker.
 */
async function waitForLeader(
    leader: WebdriverIO.Browser,
    others: WebdriverIO.Browser[],
): Promise<void> {
    await leader.waitUntil(async () => await conferenceKey(leader).isEnabled(), {
        timeoutMsg: "CONF key did not unlock for the caller, so it is not the leader",
    });
    for (const other of others) {
        if (await conferenceKey(other).isEnabled()) {
            throw new Error("CONF key is unlocked for a callee that is not the leader");
        }
    }
}

const invitation = (msg: ServerMessage) => msg.type === "callInvitation";

describe("Predefined Conference Keys", function () {
    // Three app boots and logins plus a conference with WebRTC between all
    // three apps do not fit the 60s default. Set on the suite: a per-test
    // timeout is ignored.
    this.timeout(150_000);

    let peers: SignalingTestClient[] = [];

    beforeEach(async () => {
        await resetMockState();
        for (const cid of DATAFEED_CIDS) {
            await removeController(cid);
        }
        await restartApps();
    });

    afterEach(() => {
        for (const peer of peers) {
            peer.disconnect();
        }
        peers = [];
    });

    async function connectPeer(position: string): Promise<SignalingTestClient> {
        const peer = await SignalingTestClient.connect(PEER_CID, {positionId: position});
        peers.push(peer);
        return peer;
    }

    /** The caller calls LOVV_N1 through its station key and N1 answers, with S1 idle. */
    async function callN1FromThreeApps(): Promise<void> {
        const caller = getClient("clientA");
        const n1 = getClient("clientB");

        await connect(caller, CALLER);
        await connect(n1, N1);
        await connect(getClient("clientC"), S1);

        await waitForEnabled(caller, N1_STATION_KEY, true);
        await click(caller, directAccessKey(caller, ...N1_STATION_KEY));
        await answerKey(n1).waitForDisplayed();
        await click(n1, answerKey(n1));
        await waitForCallColor(caller, directAccessKey(caller, ...N1_STATION_KEY), {active: true});
    }

    it("should ring every online station of the key and make the caller the leader", async () => {
        const caller = getClient("clientA");
        const n1 = getClient("clientB");
        const s1 = getClient("clientC");

        await connect(caller, CALLER);
        await connect(n1, N1);
        await connect(s1, S1);

        await waitForEnabled(caller, NSE_KEY, true);
        await click(caller, directAccessKey(caller, ...NSE_KEY));

        await conferenceAnswerKey(n1).waitForDisplayed();
        await conferenceAnswerKey(s1).waitForDisplayed();
        await expectPlainKey(caller, NSE_KEY, 1000);

        await click(n1, conferenceAnswerKey(n1));
        await click(s1, conferenceAnswerKey(s1));

        await waitForConferenceCallDisplay(caller);
        await waitForConferenceCallDisplay(n1);
        await waitForConferenceCallDisplay(s1);
        await waitForLeader(caller, [n1, s1]);
        await expectNoCallError(caller, 1000);

        for (const client of [caller, n1, s1]) {
            await expectPlainKey(client, NSE_KEY, 1000);
        }

        // Nothing is left to invite and N1 is no leader: the press must not leave the call.
        await click(n1, directAccessKey(n1, ...NSE_KEY));
        await expectConferenceKept({caller, n1, s1}, 1500);
    });

    it("should invite the remaining stations from a 1:1 call in conference modify mode", async () => {
        const caller = getClient("clientA");
        const n1 = getClient("clientB");
        const s1 = getClient("clientC");

        await callN1FromThreeApps();
        await expectPlainKey(caller, NSE_KEY, 1000);

        const conf = conferenceKey(caller);
        await caller.waitUntil(async () => await conf.isEnabled(), {
            timeoutMsg: "CONF key did not unlock for the established 1:1 call",
        });
        await click(caller, conf);
        await click(caller, directAccessKey(caller, ...NSE_KEY));

        await answerKey(s1).waitForDisplayed();
        await click(s1, answerKey(s1));

        await waitForConferenceCallDisplay(caller);
        await waitForConferenceCallDisplay(n1);
        await waitForConferenceCallDisplay(s1);
        await waitForLeader(caller, [n1, s1]);
        await expectNoCallError(caller, 1000);
    });

    it("should do nothing on a press outside modify mode while in a call", async () => {
        const caller = getClient("clientA");
        const n1 = getClient("clientB");
        const s1 = getClient("clientC");

        await callN1FromThreeApps();

        // LOVV_S1 is online and not in the call, so an invite would be possible.
        await click(caller, directAccessKey(caller, ...NSE_KEY));
        await expectNoIncomingCall(s1, 1500);

        await callDisplaySlot(caller).waitForDisplayed();
        await callDisplaySlot(n1).waitForDisplayed();
        await waitForCallColor(caller, directAccessKey(caller, ...N1_STATION_KEY), {active: true});
        if ((await callDisplaySlot(caller).getText()).trim() === "CONF") {
            throw new Error("The key press outside modify mode turned the call into a conference");
        }
    });

    it("should stay disabled until a listed station not covered by the caller comes online", async () => {
        const caller = getClient("clientA");

        await connect(caller, CALLER);

        // LOVV_FMP is online, but it is the caller's own.
        await waitForEnabled(caller, NSE_KEY, false);
        await waitForEnabled(caller, OFFLINE_KEY, false);

        const peer = await connectPeer("LOVV_N1_CTR");
        await waitForEnabled(caller, NSE_KEY, true);
        await waitForEnabled(caller, OFFLINE_KEY, false);

        await click(caller, directAccessKey(caller, ...NSE_KEY));

        const invite = await peer.waitForMessage(invitation);
        const target = invite.target as {station?: string};
        if (target.station !== "LOVV_N1") {
            throw new Error(`The peer was rung as ${JSON.stringify(target)}, not LOVV_N1`);
        }
        const invited = invite.invitedTargets as unknown[];
        if (invited.length !== 0) {
            throw new Error(`Offline stations were invited: ${JSON.stringify(invited)}`);
        }

        // The ringing shows on the station's own key, not on the conference key.
        await waitForPendingKey(caller, directAccessKey(caller, ...N1_STATION_KEY));
        await expectPlainKey(caller, NSE_KEY, 1000);
        // An offline station sent along would come back as TargetsNotFound.
        await expectNoCallError(caller, 1500);
    });

    it("should ring a controller covering two listed stations once without an error", async () => {
        const caller = getClient("clientA");

        await connect(caller, CALLER);
        const peer = await connectPeer("LOVV_S_CTR");

        await waitForEnabled(caller, S12_KEY, true);
        await waitForEnabled(caller, S2_STATION_KEY, true);
        await click(caller, directAccessKey(caller, ...S12_KEY));

        // The server rings under the first target in CallTarget order.
        const invite = await peer.waitForMessage(invitation);
        const target = invite.target as {station?: string};
        if (target.station !== "LOVV_S1") {
            throw new Error(`The peer was rung as ${JSON.stringify(target)}, not LOVV_S1`);
        }
        const invited = invite.invitedTargets as unknown[];
        if (invited.length !== 0) {
            throw new Error(`The peer was rung with co-targets: ${JSON.stringify(invited)}`);
        }
        await peer.expectNoMessage(msg => invitation(msg) && msg !== invite, 1500);

        // AlreadyParticipant for LOVV_S2 must drop it silently, not blink its key red.
        await waitForIdleKey(caller, directAccessKey(caller, ...S2_STATION_KEY));
        await expectPlainKey(caller, S12_KEY, 1000);
        await expectNoCallError(caller, 1000);
    });
});
