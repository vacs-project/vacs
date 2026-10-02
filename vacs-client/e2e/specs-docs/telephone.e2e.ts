import {restartApps} from "../helpers/app-control.ts";
import {resetMockState} from "../helpers/auth.ts";
import {click, getClient, waitForCallColor, waitForOutgoingCall} from "../helpers/browser.ts";
import {
    acceptIncoming,
    CALL_DISPLAY,
    CLOCK,
    CLOCK_CELL,
    closeRawClients,
    eastOrigin,
    INCOMING_ANSWER_KEY,
    inviteFrom,
    KEY_E1,
    KEY_N1,
    setupTabbed,
    TELEPHONE_BUTTON,
    waitForConnectedCall,
} from "../helpers/docs.ts";
import {captureWindow, freezeClock} from "../helpers/screenshot.ts";

const END_BUTTON = '//button[./p[text()="END"]]';

/** A tab of the telephone page, matched on its label without the line break. */
function telephoneTab(label: "Dir." | "CallList" | "DialPad" | "Ign."): string {
    return `//button[./p[.="${label}"]]`;
}

/** Sets the webview clock for the call list entry made next. */
async function setClock(browser: WebdriverIO.Browser, hhmm: string): Promise<void> {
    await freezeClock(browser, CLOCK.replace("10:10", hhmm));
}

/**
 * Ends the call on the display and waits until the display is empty, so the
 * next call starts from an idle client: a key pressed while the display still
 * holds the old call would act on that call instead.
 */
async function endCall(browser: WebdriverIO.Browser): Promise<void> {
    // An END pressed before the call reached the display ends nothing.
    await browser.$(CALL_DISPLAY).waitForDisplayed();
    await click(browser, browser.$(END_BUTTON));
    await browser.$(CALL_DISPLAY).waitForDisplayed({reverse: true});
}

describe("Documentation screenshots: telephone page", function () {
    this.timeout(180_000);

    beforeEach(async () => {
        await resetMockState();
        await restartApps();
    });

    afterEach(() => closeRawClients());

    it("captures the telephone page tabs", async () => {
        const clientA = getClient("clientA");
        const clientB = getClient("clientB");
        await setupTabbed();

        // The call list shows one entry of each kind, oldest first: an
        // outgoing call given up before it was answered, an answered outgoing
        // call, an answered incoming call, and an incoming call the caller
        // gave up on. Each at its own minute, as the list shows the time.
        await setClock(clientA, "10:05");
        await click(clientA, clientA.$(KEY_N1));
        await waitForOutgoingCall(clientA);
        await endCall(clientA);
        await clientB.$(INCOMING_ANSWER_KEY).waitForDisplayed({reverse: true});

        await setClock(clientA, "10:06");
        await click(clientA, clientA.$(KEY_N1));
        await acceptIncoming(clientB);
        await waitForCallColor(clientA, clientA.$(KEY_N1), {active: true});
        await endCall(clientA);
        await waitForCallColor(clientA, clientA.$(KEY_N1), {active: false});

        await setClock(clientA, "10:08");
        await inviteFrom(clientB, eastOrigin("LOVV_E1"), {station: "LOWW_APP"});
        await acceptIncoming(clientA);
        await waitForConnectedCall(clientA);
        await endCall(clientA);
        await waitForCallColor(clientA, clientA.$(KEY_E1), {active: false});

        await setClock(clientA, "10:10");
        await inviteFrom(clientB, eastOrigin("LOVV_E1"), {station: "LOWW_APP"});
        await clientA.$(INCOMING_ANSWER_KEY).waitForDisplayed();
        await endCall(clientB);
        await clientA.$(INCOMING_ANSWER_KEY).waitForDisplayed({reverse: true});

        const clock = clientA.$(CLOCK_CELL);
        await clientA.waitUntil(async () => (await clock.getText()).includes("10:10"), {
            timeoutMsg: "Clock did not return to the frozen time",
        });

        // The page opens on the call list.
        await click(clientA, clientA.$(TELEPHONE_BUTTON));
        await clientA.$('//p[text()="Call List"]').waitForDisplayed();
        await clientA.$('//*[text()="10:05"]').waitForDisplayed();
        await captureWindow(clientA, "interface/telephone_call_list.png");

        await click(clientA, clientA.$(telephoneTab("Dir.")));
        await clientA.$('//p[text()="Telephone Directory"]').waitForDisplayed();
        await captureWindow(clientA, "interface/telephone_dir.png");

        await click(clientA, clientA.$(telephoneTab("DialPad")));
        await clientA.$('//p[text()="Dial Pad"]').waitForDisplayed();
        await captureWindow(clientA, "interface/telephone_dial_pad.png");

        // Added through the command the Add button invokes; typing into the
        // field would only add keystroke handling to what the image shows.
        await clientA.execute(() =>
            window.__TAURI_INTERNALS__.invoke("signaling_add_ignored_client", {
                clientId: "10000002",
            }),
        );
        await click(clientA, clientA.$(telephoneTab("Ign.")));
        await clientA.$('//p[text()="Ignore List"]').waitForDisplayed();
        await clientA.$('//*[text()="10000002"]').waitForDisplayed();
        await captureWindow(clientA, "interface/telephone_ignore.png");
    });
});
