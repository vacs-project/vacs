import {
    Controller,
    loginAndConnectAs,
    removeController,
    resetMockState,
    seedController,
} from "../helpers/auth.ts";
import {click, getClient, waitForClass} from "../helpers/browser.ts";

const CID_A = "10000001";
const POSITION_A = "LOVV_E_CTR";

const HONEY = "bg-[#ffc246]";
const OWN_MARKER = "text-gray-500";

function controller(
    cid: string,
    callsign: string,
    frequency: string,
    facility: number = 6,
): Controller {
    return {
        cid: Number(cid),
        name: `Mock Controller ${cid}`,
        callsign,
        frequency,
        facility,
        rating: 0,
        server: "MOCK",
        visual_range: 50,
        text_atis: [],
        last_updated: "1970-01-01T00:00:00.000000Z",
        logon_time: "1970-01-01T00:00:00.000000Z",
    };
}

async function openGeoGroup(
    browser: WebdriverIO.Browser,
    label1: string,
    label2: string,
): Promise<void> {
    const group = await browser.$(
        `//button[.//p[@title="${label1}"] and .//p[@title="${label2}"]]`,
    );
    await group.waitForDisplayed();
    await click(browser, group);
}

function stationKey(browser: WebdriverIO.Browser, label: string): ChainablePromiseElement {
    return browser.$(`//button[.//p[@title="${label}"]]`);
}

describe("Position Dynamics", () => {
    beforeEach(async () => {
        await resetMockState();
        await multiRemoteBrowser.reloadSession();

        await loginAndConnectAs(getClient("clientA"), CID_A, POSITION_A);
    });

    it("should reassign the position when the controller changes frequency", async () => {
        const clientA = getClient("clientA");

        await openGeoGroup(clientA, "E", "APP");
        const e1 = stationKey(clientA, "E1");
        await e1.waitForDisplayed();
        await clientA.waitUntil(async () => await e1.isEnabled(), {
            timeoutMsg: "E1 did not come online",
        });
        await waitForClass(clientA, e1, OWN_MARKER, {present: true});

        // The controller switches to the LOVV_S_CTR frequency on VATSIM.
        // After the grace period the server reassigns the position: the E
        // stations fall to the remaining VATSIM-only coverage and are lost.
        await seedController(controller(CID_A, "LOVV_S_CTR", "122.865"));
        await clientA.waitUntil(async () => !(await e1.isEnabled()), {
            timeoutMsg: "E1 did not go offline after the position reassignment",
        });
        await waitForClass(clientA, e1, OWN_MARKER, {present: false});

        // The S stations are now own; the default call source follows.
        const endButton = await clientA.$("button=END");
        await click(clientA, endButton);
        await openGeoGroup(clientA, "S", "LOWG");
        const s1 = stationKey(clientA, "S1");
        await s1.waitForDisplayed();
        await clientA.waitUntil(async () => await s1.isEnabled(), {
            timeoutMsg: "S1 did not become own after the position reassignment",
        });
        await waitForClass(clientA, s1, OWN_MARKER, {present: true});
        await waitForClass(clientA, s1, HONEY, {present: true});
    });

    it("should offer position selection when the VATSIM position becomes ambiguous", async () => {
        const clientA = getClient("clientA");

        // The controller's updated callsign and frequency match two positions
        // (LOWI_E_APP and LOWI_S_APP): the server disconnects the client with
        // the ambiguous candidates and the client offers manual selection.
        await seedController(controller(CID_A, "LOWI_APP", "119.275", 5));

        const overlayTitle = await clientA.$("p=Ambiguous position");
        await overlayTitle.waitForDisplayed();

        // Resolve the ambiguity in the datafeed before reconnecting so the
        // selected position is not immediately reassigned again.
        await removeController(CID_A);

        const positionButton = await clientA.$("button=LOWI_E_APP");
        await click(clientA, positionButton);

        await overlayTitle.waitForDisplayed({reverse: true});
        const connectButton = await clientA.$("button=Connect");
        await connectButton.waitForDisplayed({reverse: true});
    });
});
