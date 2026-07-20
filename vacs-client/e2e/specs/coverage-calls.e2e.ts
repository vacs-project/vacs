import {loginAndConnectAs, removeController, resetMockState} from "../helpers/auth.ts";
import {
    callQueueSlot,
    click,
    getClient,
    waitForCallColor,
    waitForClass,
} from "../helpers/browser.ts";

const CID_A = "10000001";
// A user without a datafeed controller: keeps the explicitly chosen
// position regardless of datafeed sync.
const CID_B = "10000005";
const BC_CID = "10000003";

const TEMPORARY_SOURCE = "bg-[#ffdf9e]";
const OWN_MARKER = "text-gray-500";

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

describe("Coverage Call Routing", () => {
    beforeEach(async () => {
        await resetMockState();
        // Keep S station coverage purely client-driven: without this the
        // datafeed-only BC controller would mask the stations.
        await removeController(BC_CID);
        await multiRemoteBrowser.reloadSession();

        await loginAndConnectAs(getClient("clientA"), CID_A, "LOVV_E_CTR");
        await loginAndConnectAs(getClient("clientB"), CID_B, "LOVV_BC_CTR");
    });

    it("should route station calls to the covering client", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");

        // S1 is covered by the other client's higher-priority position.
        await openGeoGroup(clientA, "S", "LOWG");
        const s1 = stationKey(clientA, "S1");
        await s1.waitForDisplayed();
        await clientA.waitUntil(async () => await s1.isEnabled(), {
            timeoutMsg: "S1 did not come online",
        });
        await waitForClass(clientA, s1, OWN_MARKER, {present: false});

        // Calling the station reaches the covering client; the incoming call
        // is labeled with the caller's call source station (E1).
        await click(clientA, s1);
        const answerKey = callQueueSlot(clientB, "E1");
        await answerKey.waitForDisplayed();
        await click(clientB, answerKey);

        await waitForCallColor(clientA, s1, {active: true});
        await waitForCallColor(clientB, callQueueSlot(clientB, "E1"), {active: true});

        const endButton = await clientA.$("button=END");
        await click(clientA, endButton);
        await callQueueSlot(clientB, "E1").waitForDisplayed({reverse: true});

        // After the covering client leaves, the station falls through to our
        // own position: clicking it now selects a call source instead of
        // starting a call. The END button reset the geo navigation, so the
        // sector page has to be reopened first.
        await clientB.execute(async () => {
            await window.__TAURI_INTERNALS__.invoke("signaling_disconnect");
        });
        await openGeoGroup(clientA, "S", "LOWG");
        const s1After = stationKey(clientA, "S1");
        await s1After.waitForDisplayed();
        await waitForClass(clientA, s1After, OWN_MARKER, {present: true});

        await click(clientA, s1After);
        await waitForClass(clientA, s1After, TEMPORARY_SOURCE, {present: true});
        const outgoingSlot = await clientA.$('//button[contains(@class, "h-16")][.//p[@title]]');
        await outgoingSlot.waitForDisplayed({reverse: true});
    });

    it("should route position calls via the telephone directory", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");

        const telephoneButton = await clientA.$('//button[.//img[@alt="Telephone"]]');
        await click(clientA, telephoneButton);
        const directoryTab = await clientA.$("button=Dir.");
        await directoryTab.waitForDisplayed();
        await click(clientA, directoryTab);

        // Select the other client's position and call it.
        const positionRow = await clientA.$("p=LOVV_BC_CTR");
        await positionRow.waitForDisplayed();
        await click(clientA, positionRow);
        const callButton = await clientA.$("button=Call");
        await click(clientA, callButton);

        const answerKey = callQueueSlot(clientB, "E1");
        await answerKey.waitForDisplayed();
        await click(clientB, answerKey);

        await waitForCallColor(clientB, callQueueSlot(clientB, "E1"), {active: true});

        const endButton = await clientA.$("button=END");
        await click(clientA, endButton);
        await callQueueSlot(clientB, "E1").waitForDisplayed({reverse: true});
    });
});
