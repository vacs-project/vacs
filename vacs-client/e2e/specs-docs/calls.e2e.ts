import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnectAs, removeController, resetMockState} from "../helpers/auth.ts";
import {
    callQueueSlot,
    click,
    getClient,
    waitForCallColor,
    waitForOutgoingCall,
    waitForRejectedCall,
} from "../helpers/browser.ts";
import {annotate, clearAnnotations} from "../helpers/annotate.ts";
import {
    acceptIncoming,
    activeCallId,
    applyFixtures,
    CALL_DISPLAY,
    captureBlinkGif,
    CID_A,
    CID_B,
    CID_PROFILE,
    closeRawClients,
    connectRaw,
    DATAFEED_BC_CID,
    eastOrigin,
    emitEvent,
    INCOMING_ANSWER_KEY,
    inviteFrom,
    KEY_E1,
    KEY_N1,
    KEY_PRA_LW,
    KEY_VB,
    KEY_VN,
    KEY_WITHOUT_STATION,
    POSITION_A,
    setupTabbed,
    STATUS_INDICATOR,
    stationKey,
    waitForConnectedCall,
} from "../helpers/docs.ts";
import {captureWindow, copyImage} from "../helpers/screenshot.ts";

const PRIO_BUTTON = '//button[./p[.="PRIO"]]';
const RADIO_PRIO_BUTTON = '//button[./p[.="RADIOPRIO"]]';
const VB_STATION = "LOWW_APP";

describe("Documentation screenshots: calls", function () {
    this.timeout(120_000);

    beforeEach(async () => {
        await resetMockState();
        await restartApps();
    });

    afterEach(() => closeRawClients());

    it("captures the tabbed layout", async () => {
        await setupTabbed();
        const target = await captureWindow(getClient("clientA"), "using-vacs/tabbed.png");
        copyImage(target, "tabbed/tabbed.png");
    });

    it("captures an incoming call on the tabbed layout", async () => {
        const clientA = getClient("clientA");
        await setupTabbed();

        await inviteFrom(getClient("clientB"), eastOrigin("LOVV_N1"), {station: VB_STATION});
        await clientA.$(INCOMING_ANSWER_KEY).waitForDisplayed();

        const target = await captureBlinkGif(clientA, "interface/tabbed_incoming_call.gif");
        copyImage(target, "using-vacs/tabbed_incoming_call.gif");
    });

    it("captures an incoming priority call on the tabbed layout", async () => {
        const clientA = getClient("clientA");
        await setupTabbed();

        await inviteFrom(
            getClient("clientB"),
            eastOrigin("LOVV_N1"),
            {station: VB_STATION},
            {prio: true},
        );
        await clientA.$(INCOMING_ANSWER_KEY).waitForDisplayed();

        const target = await captureBlinkGif(clientA, "interface/tabbed_incoming_call_prio.gif");
        copyImage(
            target,
            "tabbed/tabbed_incoming_call_prio.gif",
            "using-vacs/tabbed_incoming_call_prio.gif",
        );
    });

    it("captures two incoming calls at once", async () => {
        const clientA = getClient("clientA");
        await setupTabbed();

        // A second caller needs its own client: one client places one call at a
        // time. LOVV_N_CTR comes before LOVV_E_CTR in E1's coverage, so the
        // claimed E1 source is still a key on the page.
        const north = await connectRaw(CID_PROFILE, "LOVV_N_CTR");
        north.inviteTarget(
            {station: VB_STATION},
            {source: {positionId: "LOVV_N_CTR", stationId: "LOVV_E1"}},
        );
        await clientA.$(INCOMING_ANSWER_KEY).waitForDisplayed();
        await inviteFrom(
            getClient("clientB"),
            eastOrigin("LOVV_N1"),
            {station: VB_STATION},
            {prio: true},
        );
        await clientA.waitUntil(
            async () => (await clientA.$$(INCOMING_ANSWER_KEY).getElements()).length === 2,
            {timeoutMsg: "The second incoming call did not reach the call queue"},
        );

        await captureBlinkGif(clientA, "using-vacs/tabbed_incoming_call_multiple.gif");
    });

    it("captures an incoming call without a target station", async () => {
        const clientA = getClient("clientA");
        await setupTabbed();

        // Called by CID, as the telephone directory does, so no station of
        // ours is the target and none gets the target highlight.
        await inviteFrom(getClient("clientB"), eastOrigin("LOVV_N1"), {client: CID_A});
        await clientA.$(INCOMING_ANSWER_KEY).waitForDisplayed();

        await captureBlinkGif(clientA, "using-vacs/tabbed_incoming_call_no_target_highlight.gif");
    });

    it("captures an outgoing call", async () => {
        const clientA = getClient("clientA");
        await setupTabbed();

        await click(clientA, clientA.$(KEY_E1));
        await getClient("clientB").$(INCOMING_ANSWER_KEY).waitForDisplayed();
        await waitForOutgoingCall(clientA);

        const target = await captureWindow(clientA, "interface/tabbed_outgoing_call.png");
        copyImage(target, "using-vacs/tabbed_outgoing_call.png");
    });

    it("captures the PRIO key and an outgoing priority call", async () => {
        const clientA = getClient("clientA");
        await setupTabbed();

        const prio = clientA.$(PRIO_BUTTON);
        await click(clientA, prio);
        await clientA.waitUntil(
            async () => ((await prio.getAttribute("class")) ?? "").includes("bg-blue-700"),
            {timeoutMsg: "PRIO did not light up"},
        );
        await captureWindow(clientA, "using-vacs/prio_call.png");

        await click(clientA, clientA.$(KEY_N1));
        await clientA.$(CALL_DISPLAY).waitForDisplayed();
        await getClient("clientB").$(INCOMING_ANSWER_KEY).waitForDisplayed();

        await captureBlinkGif(clientA, "using-vacs/tabbed_outgoing_call_prio.gif");
    });

    it("captures an established call and RADIO PRIO", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        await setupTabbed();

        await click(clientA, clientA.$(KEY_E1));
        await acceptIncoming(clientB);
        await waitForCallColor(clientA, clientA.$(KEY_E1), {active: true});
        await waitForConnectedCall(clientA);

        const target = await captureWindow(clientA, "using-vacs/tabbed_active_call.png");
        copyImage(target, "using-vacs/connection_indicator.png");

        const radioPrio = clientA.$(RADIO_PRIO_BUTTON);
        await click(clientA, radioPrio);
        await clientA.waitUntil(
            async () => ((await radioPrio.getAttribute("class")) ?? "").includes("bg-blue-700"),
            {timeoutMsg: "RADIO PRIO did not light up"},
        );
        await captureWindow(clientA, "using-vacs/radio_prio.png");
    });

    it("captures an accepted incoming call and its target station", async () => {
        const clientA = getClient("clientA");
        await setupTabbed();

        await inviteFrom(getClient("clientB"), eastOrigin("LOVV_E1"), {station: VB_STATION});
        await acceptIncoming(clientA);
        await waitForCallColor(clientA, clientA.$(KEY_E1), {active: true});
        await waitForConnectedCall(clientA);

        // The release notes list seven key states; this image carries the last,
        // the target of the accepted call.
        await annotate(clientA, [{target: KEY_VB, badge: 7, place: "top-left"}]);
        await captureWindow(clientA, "tabbed/tabbed_active_call.png");
        await clearAnnotations(clientA);
    });

    it("captures an accepted incoming priority call", async () => {
        const clientA = getClient("clientA");
        await setupTabbed();

        await inviteFrom(
            getClient("clientB"),
            eastOrigin("LOVV_N1"),
            {station: VB_STATION},
            {prio: true},
        );
        await acceptIncoming(clientA);
        await waitForConnectedCall(clientA);

        await captureWindow(clientA, "using-vacs/tabbed_active_call_prio.png");
    });

    it("captures the call source selection", async () => {
        const clientA = getClient("clientA");
        await setupTabbed();

        // VB is the position's default call source, so it already shows as the
        // fixed source; one press on VN makes that the temporary one.
        await clientA.waitUntil(
            async () => ((await clientA.$(KEY_VB).getAttribute("class")) ?? "").includes("ffc246"),
            {timeoutMsg: "VB did not show as the default call source"},
        );
        await click(clientA, clientA.$(KEY_VN));
        await clientA.waitUntil(
            async () => ((await clientA.$(KEY_VN).getAttribute("class")) ?? "").includes("ffdf9e"),
            {timeoutMsg: "VN did not become the temporary call source"},
        );

        const target = await captureWindow(clientA, "interface/tabbed_call_source.png");
        copyImage(target, "using-vacs/tabbed_call_source.png");
    });

    it("captures the direct access key states", async () => {
        const clientA = getClient("clientA");
        await setupTabbed();

        await click(clientA, clientA.$(KEY_VN));
        await clientA.waitUntil(
            async () => ((await clientA.$(KEY_VN).getAttribute("class")) ?? "").includes("ffdf9e"),
            {timeoutMsg: "VN did not become the temporary call source"},
        );

        // Numbered as in the release notes' list of key states.
        await annotate(clientA, [
            {target: KEY_WITHOUT_STATION, badge: 1},
            {target: KEY_N1, badge: 2},
            {target: KEY_PRA_LW, badge: 3},
            {target: KEY_VN, badge: 4},
            {target: KEY_VB, badge: 5},
            {target: stationKey(["APP", "VP-EC", "129050"]), badge: 6},
        ]);
        await captureWindow(clientA, "tabbed/tabbed_call_source.png");
        await clearAnnotations(clientA);
    });

    it("captures a call that was not answered", async () => {
        const clientA = getClient("clientA");
        await setupTabbed({neighbor: "silent"});

        await click(clientA, clientA.$(KEY_N1));
        await waitForOutgoingCall(clientA);

        // The client gives up on an unanswered call after a minute and reports
        // it with this event; emitting it skips the wait.
        const callId = await activeCallId("clientA");
        if (callId === null) throw new Error("No outgoing call to time out");
        await emitEvent("clientA", "webrtc:call-error", {
            callId,
            origin: {type: "targets", value: [{station: "LOVV_N1"}]},
            reason: "Remote Target did not answer",
        });
        await clientA.$('//div[@title="Remote Target did not answer"]').waitForDisplayed();

        await captureBlinkGif(clientA, "using-vacs/call_error.gif");
    });

    it("captures a rejected call", async () => {
        const clientA = getClient("clientA");
        await setupTabbed({neighbor: "rejecting"});

        await click(clientA, clientA.$(KEY_E1));
        await waitForRejectedCall(clientA);

        await captureBlinkGif(clientA, "using-vacs/call_error_rejected.gif");
    });

    it("captures a degraded call", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        const S1_KEY = '//button[.//p[@title="S1"]]';
        await removeController(DATAFEED_BC_CID);
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");
        await loginAndConnectAs(clientB, CID_B, "LOVV_BC_CTR");

        // S1 is covered by the other client's position, so calling the station
        // reaches it. The incoming call is labeled with our own call source.
        const group = await clientA.$('//button[.//p[@title="S"] and .//p[@title="LOWG"]]');
        await group.waitForDisplayed();
        await click(clientA, group);

        const s1 = clientA.$(S1_KEY);
        await s1.waitForDisplayed();
        await clientA.waitUntil(async () => await s1.isEnabled(), {
            timeoutMsg: "S1 did not come online",
        });
        await click(clientA, s1);

        const answerKey = callQueueSlot(clientB, "E1");
        await answerKey.waitForDisplayed();
        await click(clientB, answerKey);
        await waitForCallColor(clientA, s1, {active: true});

        // Wait out the real negotiation first: a call-connected event arriving
        // after the degrade event would put the call back to connected and
        // take the icon away again mid-capture.
        await waitForConnectedCall(clientA);

        // A real one-way-audio situation needs a broken network path between
        // two hosts; the event the media watchdog would emit for it does not.
        const callId = await activeCallId("clientA");
        if (callId === null) throw new Error("No active call to degrade");
        // Per peer, not per call: the store ignores an event whose peer is not
        // a joined participant. The peer is the other client, which took the
        // call as the station's coverage.
        await emitEvent("clientA", "webrtc:call-degraded", {callId, peerId: CID_B});

        await clientA.$('img[alt="No incoming audio"]').waitForDisplayed();

        await captureWindow(clientA, "troubleshooting/degraded-call.png");

        // The annotated variant marks the two symptoms the page lists, in the
        // order the prose lists them.
        await annotate(clientA, [
            // Below the indicator: the other corners cover the clock.
            {target: STATUS_INDICATOR, badge: 1, place: "bottom-right"},
            {target: '//div[img[@alt="No incoming audio"]]', badge: 2, place: "top-left"},
        ]);
        await captureWindow(clientA, "troubleshooting/degraded-call-annotated.png");
        await clearAnnotations(clientA);
    });
});
