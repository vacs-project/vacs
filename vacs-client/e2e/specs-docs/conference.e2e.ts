import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnectAs, removeController, resetMockState} from "../helpers/auth.ts";
import {
    click,
    conferenceKey,
    getClient,
    waitForCallColor,
    waitForErroredKey,
} from "../helpers/browser.ts";
import {annotate, clearAnnotations} from "../helpers/annotate.ts";
import {
    applyFixtures,
    CALL_DISPLAY,
    captureLit,
    CID_A,
    CID_B,
    CID_C,
    DATAFEED_BC_CID,
    INCOMING_ANSWER_KEY,
    POSITION_A,
    TELEPHONE_BUTTON,
    waitForConnectedCall,
    waitUntilEnabled,
} from "../helpers/docs.ts";
import {captureWindow} from "../helpers/screenshot.ts";

// The other two parties. LOVV_BC_CTR covers S1 through S7, and LOWG APP sits
// on the same direct access page as the S sectors, so all three parties of
// the captured conference are keys of one page and who answers which key is
// decided by the dataset, not by call routing luck.
const POSITION_BC = "LOVV_BC_CTR";
const POSITION_C = "LOWG_APP";

const S_GROUP = '//button[.//p[@title="S"] and .//p[@title="LOWG"]]';
const S1_KEY = '//button[.//p[@title="S1"]]';
const S2_KEY = '//button[.//p[@title="S2"]]';
const LOWG_APP_KEY = '//button[.//p[@title="LOWG"] and .//p[@title="APP"]]';
/** The call list row labeled CONF, on the telephone page. */
const CALL_LIST_CONF_ROW =
    '//div[contains(@class, "px-0.5") and contains(@class, "font-semibold")][text()="CONF"]';
/** The bottom right info grid cell, which names the last call error. */
const INFO_GRID_ERROR = '//div[@title="Remote Target participating"]';

describe("Documentation screenshots: conference calls", function () {
    this.timeout(120_000);

    beforeEach(async () => {
        await resetMockState();
        await restartApps();
    });

    it("captures a conference call and its refused target", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        const clientC = getClient("clientC");
        await removeController(DATAFEED_BC_CID);
        await restartApps();
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");
        await loginAndConnectAs(clientB, CID_B, POSITION_BC);
        await loginAndConnectAs(clientC, CID_C, POSITION_C);

        const group = await clientA.$(S_GROUP);
        await group.waitForDisplayed();
        await click(clientA, group);

        // The 1:1 call the conference grows from. Both callees answer by
        // pressing the caller's key, which carries its default call source.
        const s1 = clientA.$(S1_KEY);
        await waitUntilEnabled(clientA, s1, "S1");
        await click(clientA, s1);
        const answerB = clientB.$(INCOMING_ANSWER_KEY);
        await answerB.waitForDisplayed();
        await click(clientB, answerB);
        await waitForCallColor(clientA, s1, {active: true});
        await waitForConnectedCall(clientA);

        // CONF unlocks only once the call is established and its media
        // connected; pressing it opens modify mode, and the next key press
        // adds that sector to the call instead of starting a new one.
        const conf = conferenceKey(clientA);
        await clientA.waitUntil(async () => await conf.isEnabled(), {
            timeoutMsg: "CONF key did not unlock for the established call",
        });
        await click(clientA, conf);

        const lowgApp = clientA.$(LOWG_APP_KEY);
        await waitUntilEnabled(clientA, lowgApp, "LOWG APP");
        await click(clientA, lowgApp);
        // The invitation reaches clientC as a conference: two parties are
        // already in the call, so its answer key reads CONF.
        const answerC = clientC.$(INCOMING_ANSWER_KEY);
        await answerC.waitForDisplayed();
        await click(clientC, answerC);
        await waitForCallColor(clientA, lowgApp, {active: true});
        await waitForConnectedCall(clientA);
        // Every leg of the mesh is up: no participant carries the
        // disconnected marker the call display would show for a dead link.
        await clientA.$('img[alt="Disconnected"]').waitForDisplayed({reverse: true});

        // What's New shows the call without callouts, since it does not explain them.
        await captureWindow(clientA, "using-vacs/conference-call-plain.png");

        // The prose walks the CONF key first, then the sector that was added
        // through it.
        await annotate(clientA, [
            // Below the key: every placement above it covers the header's callsign.
            {target: '//button[@title="Conference Call"]', badge: 1, place: "bottom-right"},
            {target: LOWG_APP_KEY, badge: 2, place: "top-right"},
        ]);
        await captureWindow(clientA, "using-vacs/conference-call.png");
        await clearAnnotations(clientA);

        // The same call seen from the telephone page: the call display and
        // the call list both label a conference CONF rather than naming one
        // party.
        await click(clientA, clientA.$(TELEPHONE_BUTTON));
        await clientA.$(CALL_LIST_CONF_ROW).waitForDisplayed();
        await annotate(clientA, [
            {target: CALL_DISPLAY, badge: 1, place: "left"},
            // Below the row rather than beside it, which would cover the CIDs.
            {target: CALL_LIST_CONF_ROW, badge: 2, place: "bottom-right"},
        ]);
        await captureWindow(clientA, "using-vacs/conference-call-list.png");
        await clearAnnotations(clientA);
        await click(clientA, clientA.$(TELEPHONE_BUTTON));
        await clientA.$(CALL_LIST_CONF_ROW).waitForDisplayed({reverse: true});

        // A target the server refuses. S2 is covered by the same controller
        // that already answered S1, so the invite comes back as a refusal
        // rather than ringing anywhere: the key is annotated and the reason
        // lands in the info grid.
        await click(clientA, conf);
        const s2 = clientA.$(S2_KEY);
        await waitUntilEnabled(clientA, s2, "S2");
        await click(clientA, s2);
        await waitForErroredKey(clientA, s2);
        await clientA.$(INFO_GRID_ERROR).waitForDisplayed();

        await annotate(clientA, [
            {target: S2_KEY, badge: 1, place: "top-right"},
            // The cell sits in the window's top right corner, so the badge goes
            // to its left; above it would be cut off by the window edge.
            {target: INFO_GRID_ERROR, badge: 2, place: "left"},
        ]);
        await captureLit(clientA, "using-vacs/conference-refused-target.png", {
            isLit: async () =>
                ((await clientA.$(S2_KEY).getAttribute("class")) ?? "").includes("bg-red-500"),
        });
        await clearAnnotations(clientA);
    });
});
