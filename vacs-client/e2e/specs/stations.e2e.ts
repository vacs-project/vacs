import {restartApps} from "../helpers/app-control.ts";
import {
    Controller,
    loginAndConnectAs,
    removeController,
    resetMockState,
    seedController,
} from "../helpers/auth.ts";
import {click, getClient, waitForClass} from "../helpers/browser.ts";

const CID_A = "10000001";
// A user without a datafeed controller: keeps any explicitly chosen
// position regardless of datafeed sync.
const CID_B = "10000005";
const POSITION_A = "LOVV_E_CTR";

// The seeded 132.950 controller resolves to position LOVV_BC_CTR (see
// seed.json and the LO dataset). Station LOVV_S1 is covered by BC before E
// in its controlled_by priority order.
const BC_CID = "10000003";

const HONEY = "bg-[#ffc246]";
const TEMPORARY_SOURCE = "bg-[#ffdf9e]";
const OWN_MARKER = "text-gray-500";

function controller(cid: string, callsign: string, frequency: string): Controller {
    return {
        cid: Number(cid),
        name: `Mock Controller ${cid}`,
        callsign,
        frequency,
        facility: 6,
        rating: 0,
        server: "MOCK",
        visual_range: 50,
        text_atis: [],
        last_updated: "1970-01-01T00:00:00.000000Z",
        logon_time: "1970-01-01T00:00:00.000000Z",
    };
}

/**
 * Opens a geo page group by the two label lines of its group button
 * (e.g. "E" / "APP" for the eastern sector page).
 */
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

/** Returns the station key button carrying the given label line. */
function stationKey(browser: WebdriverIO.Browser, label: string): ChainablePromiseElement {
    return browser.$(`//button[.//p[@title="${label}"]]`);
}

describe("Station Keys", () => {
    beforeEach(async () => {
        await resetMockState();
        await restartApps();

        await loginAndConnectAs(getClient("clientA"), CID_A, POSITION_A);
    });

    it("should show own stations online with an automatic default call source", async () => {
        const clientA = getClient("clientA");

        await openGeoGroup(clientA, "E", "APP");

        // E1 is own (connected as LOVV_E_CTR) and online via the seeded
        // datafeed controllers, and is auto-selected as default call source.
        const e1 = stationKey(clientA, "E1");
        await e1.waitForDisplayed();
        await clientA.waitUntil(async () => await e1.isEnabled(), {
            timeoutMsg: "E1 did not come online",
        });
        await waitForClass(clientA, e1, HONEY, {present: true});
        await waitForClass(clientA, e1, OWN_MARKER, {present: true});

        // E2 is own and online too, but not the default call source.
        const e2 = stationKey(clientA, "E2");
        await clientA.waitUntil(async () => await e2.isEnabled(), {
            timeoutMsg: "E2 did not come online",
        });
        await waitForClass(clientA, e2, OWN_MARKER, {present: true});
        await waitForClass(clientA, e2, HONEY, {present: false});
    });

    it("should mask stations covered only by VATSIM controllers", async () => {
        const clientA = getClient("clientA");

        await openGeoGroup(clientA, "S", "LOWG");

        // S1's highest-priority online position is LOVV_BC_CTR, which is only
        // staffed by a datafeed controller (no vacs client), so the station
        // is not callable.
        const s1 = stationKey(clientA, "S1");
        await s1.waitForDisplayed();
        await clientA.waitUntil(async () => !(await s1.isEnabled()), {
            timeoutMsg: "S1 should be masked by the VATSIM-only BC position",
        });

        // Once BC leaves the datafeed, coverage falls through to our own
        // LOVV_E_CTR and the station enters vacs coverage.
        await removeController(BC_CID);
        await clientA.waitUntil(async () => await s1.isEnabled(), {
            timeoutMsg: "S1 did not come online after BC left the datafeed",
        });
        await waitForClass(clientA, s1, OWN_MARKER, {present: true});

        // Select the now-own station as temporary call source.
        await click(clientA, s1);
        await waitForClass(clientA, s1, TEMPORARY_SOURCE, {present: true});

        // When BC reappears in the datafeed it masks the station again: the
        // previous controller loses the station and its source selection.
        await seedController(controller(BC_CID, "LOVV_BC_CTR", "132.950"));
        await clientA.waitUntil(async () => !(await s1.isEnabled()), {
            timeoutMsg: "S1 did not go offline after BC rejoined the datafeed",
        });
        await waitForClass(clientA, s1, TEMPORARY_SOURCE, {present: false});
        await waitForClass(clientA, s1, OWN_MARKER, {present: false});
    });

    it("should hand off stations when covering clients connect and disconnect", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");

        await openGeoGroup(clientA, "S", "LOWG");

        // Remove the datafeed-only BC controller so S1 is owned by our own
        // position via coverage fall-through.
        await removeController(BC_CID);
        const s1 = stationKey(clientA, "S1");
        await s1.waitForDisplayed();
        await clientA.waitUntil(async () => await s1.isEnabled(), {
            timeoutMsg: "S1 did not come online after BC left the datafeed",
        });
        await waitForClass(clientA, s1, OWN_MARKER, {present: true});

        // A second client connects as LOVV_BC_CTR, which outranks LOVV_E_CTR
        // in S1's coverage priority: the station hands off to them.
        await loginAndConnectAs(clientB, CID_B, "LOVV_BC_CTR");
        await waitForClass(clientA, s1, OWN_MARKER, {present: false});
        await clientA.waitUntil(async () => await s1.isEnabled(), {
            timeoutMsg: "S1 went offline instead of handing off to BC",
        });

        // When they disconnect, the station hands back.
        await clientB.execute(async () => {
            await window.__TAURI_INTERNALS__.invoke("signaling_disconnect");
        });
        await waitForClass(clientA, s1, OWN_MARKER, {present: true});
    });

    it("should provide late-joining clients with the current coverage", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");

        // Change coverage before the second client joins: S1 falls through
        // from the (removed) VATSIM-only BC position to LOVV_E_CTR.
        await removeController(BC_CID);
        await openGeoGroup(clientA, "S", "LOWG");
        const s1A = stationKey(clientA, "S1");
        await s1A.waitForDisplayed();
        await clientA.waitUntil(async () => await s1A.isEnabled(), {
            timeoutMsg: "S1 did not come online after BC left the datafeed",
        });

        // A client joining now must see that state from its initial station
        // list: S1 online and owned by the other client's position.
        await loginAndConnectAs(clientB, CID_B, "LOVV_N_CTR");
        await openGeoGroup(clientB, "S", "LOWG");
        const s1B = stationKey(clientB, "S1");
        await s1B.waitForDisplayed();
        await clientB.waitUntil(async () => await s1B.isEnabled(), {
            timeoutMsg: "Late-joining client did not receive S1 as online",
        });
        await waitForClass(clientB, s1B, OWN_MARKER, {present: false});
    });

    it("should keep shared positions online while one client remains", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");

        // Both clients hold the same position.
        await loginAndConnectAs(clientB, CID_B, POSITION_A);
        await openGeoGroup(clientB, "E", "APP");
        const e1 = stationKey(clientB, "E1");
        await e1.waitForDisplayed();
        await clientB.waitUntil(async () => await e1.isEnabled(), {
            timeoutMsg: "E1 did not come online for the second client",
        });
        await waitForClass(clientB, e1, OWN_MARKER, {present: true});

        // One client leaving must not take the shared position offline.
        await clientA.execute(async () => {
            await window.__TAURI_INTERNALS__.invoke("signaling_disconnect");
        });
        await clientB.pause(2000);
        await clientB.waitUntil(async () => await e1.isEnabled(), {
            timeoutMsg: "E1 went offline although the position is still covered",
        });
        await waitForClass(clientB, e1, OWN_MARKER, {present: true});
    });
});
