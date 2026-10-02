import path from "node:path";
import {fileURLToPath} from "node:url";
import {restartApps} from "../helpers/app-control.ts";
import {authenticate, loginAndConnect, loginAndConnectAs, resetMockState} from "../helpers/auth.ts";
import {
    click,
    frequencyObjects,
    getClient,
    mockCommand,
    pageCycleButton,
    pageCycleCell,
    pageTab,
    splitResizeHandle,
    waitForOutgoingCall,
} from "../helpers/browser.ts";
import {annotate, ANNOTATION_COLORS, clearAnnotations} from "../helpers/annotate.ts";
import {
    activeCallId,
    applyFixtures,
    applyTrackAudioMocks,
    captureLit,
    CID_A,
    CID_B,
    CID_C,
    CID_D,
    CID_PROFILE,
    CLOCK_CELL,
    closeRawClients,
    connectRaw,
    emitEvent,
    KEY_E1,
    KEY_N1,
    KEY_PRA_LW,
    KEY_WITHOUT_STATION,
    loadTestProfile,
    MISSION_BUTTON,
    NEXT_VERSION,
    POSITION_A,
    radioStation,
    setupTabbed,
    stationKey,
    STATUS_INDICATOR,
    TABBED_POSITION,
    tabButton,
    VERSION,
} from "../helpers/docs.ts";
import {captureElement, captureRect, captureWindow, viewportRect} from "../helpers/screenshot.ts";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

const SPLIT_PROFILE = path.resolve(__dirname, "..", "fixtures", "profile-docs-split.json");
const CYCLE_PROFILE = path.resolve(__dirname, "..", "fixtures", "profile-cycle.json");
const SIX_TABS_PROFILE = path.resolve(__dirname, "..", "fixtures", "profile-docs-six-tabs.json");

/** A cell of the header's info grid, matched by the text it shows in full in its title. */
function infoCell(title: string): string {
    return `//div[contains(@class, "info-grid-cell")][@title="${title}"]`;
}

const UPDATE_CELL = infoCell(`UPDATE AVAILABLE (v${NEXT_VERSION})`);
/** The grid of direct access keys of the selected tab. */
const DIRECT_ACCESS_GRID = '(//div[contains(@class, "grid-flow-col")])[1]';
/** The row of tab buttons in the bottom right. */
const TAB_ROW = `${tabButton("EC")}/../..`;
/** The button cycling through tab sets when a profile has more than four tabs. */
const TAB_SET_BUTTON = '//button[.//img[@alt="<->"]]';

/** The strip of the window below the header, which the connect page image shows. */
async function belowHeader(browser: WebdriverIO.Browser) {
    const viewport = await viewportRect(browser);
    const top = await browser.execute(
        (selector: string) =>
            (
                document.evaluate(
                    selector,
                    document,
                    null,
                    XPathResult.FIRST_ORDERED_NODE_TYPE,
                    null,
                ).singleNodeValue as Element
            ).getBoundingClientRect().bottom,
        CLOCK_CELL,
    );
    return {...viewport, y: top, height: viewport.height - top};
}

/** The bottom `height` CSS pixels of the window. */
async function bottomStrip(browser: WebdriverIO.Browser, height: number) {
    const viewport = await viewportRect(browser);
    return {...viewport, y: viewport.height - height, height};
}

describe("Documentation screenshots: interface", function () {
    this.timeout(120_000);

    beforeEach(async () => {
        await resetMockState();
        await restartApps();
    });

    afterEach(() => closeRawClients());

    it("captures the login page", async () => {
        const clientA = getClient("clientA");
        await clientA.$("button=Login via VATSIM").waitForDisplayed();
        await applyFixtures(clientA, "clientA");

        await captureWindow(clientA, "using-vacs/login.png");
    });

    it("captures the connect page and the update notification", async () => {
        const clientA = getClient("clientA");
        await authenticate(clientA, CID_A);
        await clientA.$("button=Connect").waitForDisplayed();
        await applyFixtures(clientA, "clientA");

        await captureRect(clientA, "using-vacs/connect.png", await belowHeader(clientA));

        await applyFixtures(clientA, "clientA", {newVersion: NEXT_VERSION});
        await clientA.$(UPDATE_CELL).waitForDisplayed();
        await annotate(clientA, [{target: UPDATE_CELL}]);
        await captureWindow(clientA, "getting-started/update_available.png");
        await captureRect(clientA, "using-vacs/update_not.png", {
            ...(await viewportRect(clientA)),
            height: 288,
        });
        await clearAnnotations(clientA);
    });

    it("captures the mission page", async () => {
        const clientA = getClient("clientA");
        await authenticate(clientA, CID_A);
        await clientA.$("button=Connect").waitForDisplayed();
        await applyFixtures(clientA, "clientA");

        await click(clientA, clientA.$(MISSION_BUTTON));
        const mission = clientA.$('//p[text()="Mission"]');
        await mission.waitForDisplayed();
        await captureWindow(clientA, "using-vacs/mission.png");

        // Loaded through the backend command with a path, since the button
        // opens a native file dialog. The button is pressed anyway, with the
        // dialog mocked, so the page also holds the path its reload and unload
        // buttons need.
        await loadTestProfile(clientA, SIX_TABS_PROFILE);
        await mockCommand("clientA", "app_load_test_profile", {resolve: SIX_TABS_PROFILE});
        // Loading a profile leaves the mission page for the profile's phone page.
        await clientA.waitUntil(
            async () => {
                if (await clientA.$('//p[text()="Mission"]').isDisplayed()) return true;
                await click(clientA, clientA.$(MISSION_BUTTON));
                return false;
            },
            {timeoutMsg: "The mission page did not reopen"},
        );
        await click(clientA, clientA.$('//button[.//p[text()="Load test profile"]]'));
        await clientA.$('//*[text()="Unavailable while using a profile"]').waitForDisplayed();
        const reload = clientA.$('//button[.//img[@alt="Reload"]]');
        await clientA.waitUntil(async () => await reload.isEnabled(), {
            timeoutMsg: "The test profile buttons did not unlock",
        });
        await captureWindow(clientA, "using-vacs/mission_test_profile.png");
    });

    it("captures the top status bar", async () => {
        const clientA = getClient("clientA");
        await setupTabbed({neighbor: "silent"});
        await applyFixtures(clientA, "clientA", {newVersion: NEXT_VERSION});

        // A call that went unanswered, for the error reason in the header.
        await click(clientA, clientA.$(KEY_N1));
        await waitForOutgoingCall(clientA);
        const callId = await activeCallId("clientA");
        if (callId === null) throw new Error("No outgoing call to time out");
        await emitEvent("clientA", "webrtc:call-error", {
            callId,
            origin: {type: "targets", value: [{station: "LOVV_N1"}]},
            reason: "Remote Target did not answer",
        });
        const errorCell = infoCell("Remote Target did not answer");
        await clientA.$(errorCell).waitForDisplayed();

        // The cells span the header's width, so the callouts anchor on their
        // text instead: badges beside the cell would sit in the next cell.
        const cells = {
            cid: infoCell(CID_A),
            position: infoCell(TABBED_POSITION),
            version: infoCell(`Version: v${VERSION}`),
            update: UPDATE_CELL,
            error: errorCell,
        };
        await clientA.execute((selectors: Record<string, string>) => {
            for (const [name, selector] of Object.entries(selectors)) {
                const cell = document.evaluate(
                    selector,
                    document,
                    null,
                    XPathResult.FIRST_ORDERED_NODE_TYPE,
                    null,
                ).singleNodeValue as Element;
                const span = document.createElement("span");
                span.dataset.docsCallout = name;
                span.append(...Array.from(cell.childNodes));
                cell.append(span);
            }
        }, cells);
        const text = (name: string) => `//span[@data-docs-callout="${name}"]`;

        // In the order the page lists the bar's contents.
        await annotate(clientA, [
            {target: `${CLOCK_CELL}/div[1]/p`, badge: 1, place: "right", box: false},
            {target: text("cid"), badge: 2, place: "left", box: false},
            {target: text("position"), badge: 3, place: "left", box: false},
            {target: text("version"), badge: 4, place: "left", box: false},
            {target: text("update"), badge: 5, place: "left", box: false},
            {target: STATUS_INDICATOR, badge: 6, place: "below", box: false},
            {target: text("error"), badge: 7, place: "left", box: false},
        ]);
        await captureLit(clientA, "interface/topbar.png", {
            rect: {...(await viewportRect(clientA)), height: 280},
        });
        await clearAnnotations(clientA);
    });

    it("captures the direct access page", async () => {
        const clientA = getClient("clientA");
        await setupTabbed();

        // Numbered as in the overview page's list of key states; the two
        // unnumbered boxes mark the page and one key, the terms it defines.
        await annotate(clientA, [
            {target: DIRECT_ACCESS_GRID, color: ANNOTATION_COLORS.blue},
            {
                target: stationKey(["APP", "VP-EC", "129050"]),
                color: ANNOTATION_COLORS.violet,
            },
            {target: KEY_WITHOUT_STATION, badge: 1},
            {target: KEY_E1, badge: 2},
            {target: KEY_PRA_LW, badge: 3},
            {target: stationKey(["APP", "VM-EC", "125175"]), badge: 4},
        ]);
        await captureWindow(clientA, "interface/directaccesspage.png");
        await clearAnnotations(clientA);
    });

    it("captures the tabbed layout overview and its tabs", async () => {
        const clientA = getClient("clientA");
        await setupTabbed();

        await annotate(clientA, [
            {target: KEY_E1, badge: 1, place: "top-right"},
            {target: DIRECT_ACCESS_GRID, badge: 2, place: "top-left"},
            {target: TAB_ROW, badge: 3, place: "top-left"},
        ]);
        await captureWindow(clientA, "interface/tabbed_annot.png");
        await clearAnnotations(clientA);

        await annotate(clientA, [{target: TAB_ROW}]);
        await captureRect(clientA, "interface/tabs.png", await bottomStrip(clientA, 444));
        await clearAnnotations(clientA);
    });

    it("captures the bottom control bar", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");

        await captureRect(clientA, "interface/bottom.png", await bottomStrip(clientA, 133));
    });

    it("captures the tab set selector of a profile with six tabs", async () => {
        const clientA = getClient("clientA");
        // Online neighbors, so the keys above the tab row look like they do
        // on a staffed position.
        await connectRaw(CID_B, POSITION_A);
        await connectRaw(CID_C, "LOWW_D_APP");
        await connectRaw(CID_D, "LOWW_I_APP");
        await loginAndConnect(clientA, CID_PROFILE);
        await applyFixtures(clientA, "clientA");
        await loadTestProfile(clientA, SIX_TABS_PROFILE);

        const selector = clientA.$(TAB_SET_BUTTON);
        await selector.waitForDisplayed();
        await clientA.$(KEY_E1).waitForDisplayed();
        await annotate(clientA, [{target: TAB_SET_BUTTON}]);
        await captureRect(clientA, "interface/dasel.png", await bottomStrip(clientA, 442));
        await clearAnnotations(clientA);

        await click(clientA, selector);
        await clientA.$(tabButton("DA 5")).waitForDisplayed();
        await captureRect(clientA, "interface/da56.png", await bottomStrip(clientA, 384));
    });

    it("captures the mixed page of a split view profile", async () => {
        const clientA = getClient("clientA");
        await loginAndConnect(clientA, CID_PROFILE);
        await applyFixtures(clientA, "clientA");
        await applyTrackAudioMocks("clientA", "Connected", [
            radioStation("LOVV_CTR", 134_350_000, {rx: true, tx: true}),
            radioStation("LOWW_APP", 134_675_000, {rx: true}),
            radioStation("LOWW_TWR", 119_400_000),
        ]);
        await loadTestProfile(clientA, SPLIT_PROFILE);

        await pageTab(clientA, "Radio").waitForDisplayed();
        await splitResizeHandle(clientA).waitForExist();
        await clientA.waitUntil(
            async () => (await frequencyObjects(clientA).getElements()).length === 3,
            {timeoutMsg: "The radio pane did not show the three mocked stations"},
        );

        // The handle is opacity-0 until hovered, and the driver cannot hover:
        // it synthesizes events rather than moving a pointer.
        const handle = await splitResizeHandle(clientA);
        await clientA.execute((el: HTMLElement) => {
            el.style.opacity = "0.3";
        }, handle);
        await captureWindow(clientA, "interface/split_view_mixed.png");
    });

    it("captures the Page button of a cycle view profile", async () => {
        const clientA = getClient("clientA");
        await loginAndConnect(clientA, CID_PROFILE);
        await applyFixtures(clientA, "clientA");
        await applyTrackAudioMocks("clientA", "Disconnected");
        await loadTestProfile(clientA, CYCLE_PROFILE);

        const cycleButton = pageCycleButton(clientA);
        await cycleButton.waitForDisplayed();
        await clientA.waitUntil(
            async () =>
                ((await pageCycleCell(clientA, "M").getAttribute("class")) ?? "").includes(
                    "bg-gray-400",
                ),
            {timeoutMsg: "The Page button did not highlight the mixed page"},
        );

        await captureElement(clientA, cycleButton, "interface/page_cycle_button.png", {padding: 8});
    });
});
