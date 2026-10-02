import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnectAs, resetMockState} from "../helpers/auth.ts";
import {click, getClient} from "../helpers/browser.ts";
import {
    applyFixtures,
    captureBlinkGif,
    CID_A,
    CID_C,
    CID_D,
    closeRawClients,
    connectRaw,
    INCOMING_ANSWER_KEY,
    POSITION_A,
    stationKey,
    waitUntilEnabled,
} from "../helpers/docs.ts";
import type {SignalingTestClient} from "../helpers/signaling-client.ts";
import {captureWindow, copyImage} from "../helpers/screenshot.ts";

const E_APP_GROUP = stationKey(["E", "APP"]);
const KEY_APP_VB = stationKey(["APP", "VB", "PLN"]);
const KEY_APP_VD2 = stationKey(["APP", "VD2", "PLN"]);
const KEY_E1 = stationKey(["310-", "E1", "PLC"]);
const KEY_E2 = stationKey(["320-340", "E2", "PLC"]);

/**
 * Connects clientA on its geo position with the Vienna approach staffed, so
 * the E / APP page shows its APP and LOWW keys online. Returns the LOWW_APP
 * client, which places the incoming calls.
 */
async function setupGeo(): Promise<SignalingTestClient> {
    const clientA = getClient("clientA");
    const approach = await connectRaw(CID_C, "LOWW_APP");
    await connectRaw(CID_D, "LOWW_D_APP");
    await loginAndConnectAs(clientA, CID_A, POSITION_A);
    await applyFixtures(clientA, "clientA");
    await clientA.$(E_APP_GROUP).waitForDisplayed();
    return approach;
}

async function openEastPage(browser: WebdriverIO.Browser): Promise<void> {
    await click(browser, browser.$(E_APP_GROUP));
    await waitUntilEnabled(browser, browser.$(KEY_APP_VB), "APP VB");
    await waitUntilEnabled(browser, browser.$(KEY_APP_VD2), "APP VD2");
}

describe("Documentation screenshots: geo layout", function () {
    this.timeout(120_000);

    beforeEach(async () => {
        await resetMockState();
        await restartApps();
    });

    afterEach(() => closeRawClients());

    it("captures the geo layout and a sector page", async () => {
        const clientA = getClient("clientA");
        await setupGeo();

        const top = await captureWindow(clientA, "interface/geo.png");
        copyImage(top, "geo/geo.png", "using-vacs/geo.png");

        await openEastPage(clientA);
        const page = await captureWindow(clientA, "interface/geo_page.png");
        copyImage(page, "geo/geo_page.png");

        // E1 is the position's default call source; one press on E2 makes
        // that the temporary one.
        await click(clientA, clientA.$(KEY_E2));
        await clientA.waitUntil(
            async () => ((await clientA.$(KEY_E2).getAttribute("class")) ?? "").includes("ffdf9e"),
            {timeoutMsg: "E2 did not become the temporary call source"},
        );
        await captureWindow(clientA, "interface/geo_page_call_source.png");
    });

    it("captures an incoming call on the geo layout", async () => {
        const clientA = getClient("clientA");
        const approach = await setupGeo();

        // LOWL APP falls back to LOVV_E_CTR while unstaffed, so the call to it
        // highlights the N / LOWL group as the target and the E / APP group,
        // home of the caller's VB key, as the source.
        approach.inviteTarget(
            {station: "LOWL_APP"},
            {source: {positionId: "LOWW_APP", stationId: "LOWW_APP"}},
        );
        await clientA.$(INCOMING_ANSWER_KEY).waitForDisplayed();

        await captureBlinkGif(clientA, "interface/geo_incoming_call.gif");
    });

    it("captures an incoming call on a geo sector page", async () => {
        const clientA = getClient("clientA");
        const approach = await setupGeo();
        await openEastPage(clientA);

        approach.inviteTarget(
            {station: "LOVV_E1"},
            {source: {positionId: "LOWW_APP", stationId: "LOWW_APP"}},
        );
        await clientA.$(INCOMING_ANSWER_KEY).waitForDisplayed();
        await clientA.$(KEY_E1).waitForDisplayed();

        await captureBlinkGif(clientA, "interface/geo_page_incoming_call.gif");
    });
});
