import path from "node:path";
import {fileURLToPath} from "node:url";
import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {
    click,
    expectOverflowTabStays,
    frequencyObjects,
    getClient,
    type OverflowEdge,
    overflowTab,
    pageTab,
    scrollAreaIn,
    splitPhonePane,
    splitRadioPane,
    splitResizeHandle,
    waitForOverflowTab,
} from "../helpers/browser.ts";
import {applyTrackAudioMocks, loadTestProfile, radioStation} from "../helpers/docs.ts";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

// Positionless, so the server never pushes a profile over the test profile.
const CID_A = "10000004";

const OVERFLOW_PROFILE = path.resolve(__dirname, "..", "fixtures", "profile-overflow.json");

type Pane = () => ChainablePromiseElement;

type Geometry = {
    offset: number;
    max: number;
    /** Distance of every item from the edge the area starts at, in layout px. */
    insets: number[];
    gap: number;
};

/** The first key of a tab of the overflow fixture, keyed by the tab's key prefix. */
function firstKey(prefix: "W" | "T" | "FIT"): string {
    const row = prefix === "FIT" ? "R2" : "R1";
    return `//button[.//p[@title="${prefix}"] and .//p[@title="${row}"] and .//p[@title="C1"]]`;
}

async function changeZoom(
    browser: WebdriverIO.Browser,
    change: "increase" | "decrease" | "reset",
): Promise<void> {
    await browser.execute(
        (zoomChange: string) =>
            window.__TAURI_INTERNALS__.invoke("app_change_zoom_level", {change: zoomChange}),
        change,
    );
}

async function geometry(
    browser: WebdriverIO.Browser,
    pane: Pane,
    axis: "x" | "y",
    reversed = false,
): Promise<Geometry> {
    return browser.execute(
        (scroller: HTMLElement, horizontal: boolean, fromBottom: boolean) => {
            const grid = scroller.firstElementChild as HTMLElement;
            const area = scroller.getBoundingClientRect();
            const scale = area.width / scroller.offsetWidth;
            const style = getComputedStyle(grid);
            return {
                offset: Math.abs(horizontal ? scroller.scrollLeft : scroller.scrollTop),
                max: horizontal
                    ? scroller.scrollWidth - scroller.clientWidth
                    : scroller.scrollHeight - scroller.clientHeight,
                insets: Array.from(grid.children, item => {
                    const rect = item.getBoundingClientRect();
                    if (horizontal) return (rect.left - area.left) / scale;
                    const distance = fromBottom ? area.bottom - rect.bottom : rect.top - area.top;
                    return distance / scale;
                }),
                gap: parseFloat(horizontal ? style.columnGap : style.rowGap),
            };
        },
        await scrollAreaIn(pane()),
        axis === "x",
        reversed,
    );
}

/** Clicks a tab and returns the geometry once the smooth scroll has come to rest. */
async function clickTab(
    browser: WebdriverIO.Browser,
    pane: Pane,
    edge: OverflowEdge,
    reversed = false,
): Promise<Geometry> {
    const axis = edge === "left" || edge === "right" ? "x" : "y";
    const before = (await geometry(browser, pane, axis, reversed)).offset;
    await click(browser, overflowTab(pane(), edge));

    let last = before;
    let current = await geometry(browser, pane, axis, reversed);
    await browser.waitUntil(
        async () => {
            current = await geometry(browser, pane, axis, reversed);
            const resting = current.offset !== before && current.offset === last;
            last = current.offset;
            return resting;
        },
        {interval: 150, timeoutMsg: `The ${edge} tab did not scroll the pane`},
    );
    return current;
}

/** An item sits one gap behind the start edge, which is how the unscrolled page looks. */
function expectLinedUp({insets, gap, offset}: Geometry, what: string): void {
    if (!insets.some(inset => Math.abs(inset - gap) <= 1.5)) {
        const nearest = insets.reduce((a, b) => (Math.abs(a - gap) < Math.abs(b - gap) ? a : b));
        throw new Error(
            `${what}: no item lines up at offset ${offset} (nearest inset ${nearest}, gap ${gap})`,
        );
    }
}

// The webview can come to rest this far short of the end when zoomed out.
const END_TOLERANCE_PX = 4;

/** Steps to the far end one tab click at a time, then one click back. */
async function walk(
    browser: WebdriverIO.Browser,
    pane: Pane,
    forward: OverflowEdge,
    back: OverflowEdge,
    reversed = false,
): Promise<void> {
    const offsets = [0];
    let current: Geometry;
    do {
        current = await clickTab(browser, pane, forward, reversed);
        offsets.push(current.offset);
        if (current.offset < current.max - END_TOLERANCE_PX) {
            expectLinedUp(current, `After clicking the ${forward} tab`);
        }
    } while (current.offset < current.max - END_TOLERANCE_PX && offsets.length < 30);

    // A click that jumped straight to the end would pass every check above.
    if (offsets.length < 3) {
        throw new Error(`The ${forward} tab reached the end in a single click`);
    }
    await waitForOverflowTab(browser, pane, forward, false, `The ${forward} tab stayed at the end`);

    const returned = await clickTab(browser, pane, back, reversed);
    expectLinedUp(returned, `After clicking the ${back} tab at the end`);
}

/** Clicks a tab until nothing is hidden behind its edge anymore. */
async function rewind(
    browser: WebdriverIO.Browser,
    pane: Pane,
    edge: OverflowEdge,
    reversed = false,
): Promise<void> {
    while (await overflowTab(pane(), edge).isEnabled()) {
        await clickTab(browser, pane, edge, reversed);
    }
}

describe("Overflow cues", function () {
    this.timeout(120_000);

    beforeEach(async () => {
        await resetMockState();
        await restartApps();
        await loginAndConnect(getClient("clientA"), CID_A);
    });

    async function openOverflowProfile(browser: WebdriverIO.Browser, stations: number) {
        await applyTrackAudioMocks(
            "clientA",
            "VoiceConnected",
            Array.from({length: stations}, (_, i) =>
                radioStation(`LOVV_S${i + 1}`, 134_000_000 + i * 25_000),
            ),
        );
        await loadTestProfile(browser, OVERFLOW_PROFILE);
        await splitResizeHandle(browser).waitForExist();
        await browser.waitUntil(
            async () => (await frequencyObjects(browser).getElements()).length === stations,
            {timeoutMsg: `The radio pane did not show the ${stations} mocked stations`},
        );
        await browser.$(firstKey("W")).waitForExist();
    }

    async function selectTab(browser: WebdriverIO.Browser, label: string, key: string) {
        await click(browser, pageTab(browser, label));
        await browser.$(key).waitForExist();
    }

    it("should mark hidden content on the radio and phone panes", async () => {
        const clientA = getClient("clientA");
        await openOverflowProfile(clientA, 24);

        const radio = () => splitRadioPane(clientA);
        await waitForOverflowTab(clientA, radio, "top", true, "No tab for the stations above");
        await expectOverflowTabStays(radio, "bottom", false, 500, "below the first radio row");

        const phone = () => splitPhonePane(clientA);
        await waitForOverflowTab(clientA, phone, "right", true, "No tab for the keys on the right");
        await selectTab(clientA, "FIT", firstKey("FIT"));
        await waitForOverflowTab(clientA, phone, "right", false, "The FIT tab kept the right tab");
        await expectOverflowTabStays(phone, "right", false, 500, "on the FIT tab");
    });

    it("should refresh the tabs when a page with the same key count but different rows is selected", async () => {
        const clientA = getClient("clientA");
        await openOverflowProfile(clientA, 3);

        // WIDE and TALL both have 48 keys with three label lines, so switching
        // between them patches the same key buttons instead of replacing them.
        const phone = () => splitPhonePane(clientA);
        for (let round = 1; round <= 2; round++) {
            await waitForOverflowTab(clientA, phone, "right", true, `WIDE, round ${round}`);

            await selectTab(clientA, "TALL", firstKey("T"));
            await waitForOverflowTab(clientA, phone, "bottom", true, `TALL, round ${round}`);
            await waitForOverflowTab(clientA, phone, "right", false, `TALL, round ${round}`);
            await expectOverflowTabStays(phone, "right", false, 500, `on TALL, round ${round}`);

            await selectTab(clientA, "WIDE", firstKey("W"));
        }
    });

    it("should scroll the direct access keys one column or row per tab click", async () => {
        const clientA = getClient("clientA");
        await openOverflowProfile(clientA, 3);
        const phone = () => splitPhonePane(clientA);

        await waitForOverflowTab(clientA, phone, "right", true, "No right tab on WIDE");
        await walk(clientA, phone, "right", "left");

        // The divider overlaps the pane's left edge and must not cover the tab.
        const covered = await clientA.execute(
            (tab: HTMLElement) => {
                const rect = tab.getBoundingClientRect();
                const hit = document.elementFromPoint(rect.left + 1, rect.top + rect.height / 2);
                return !tab.contains(hit);
            },
            await overflowTab(phone(), "left"),
        );
        if (covered) throw new Error("Something covers the left edge of the left tab");

        await selectTab(clientA, "TALL", firstKey("T"));
        await waitForOverflowTab(clientA, phone, "bottom", true, "No bottom tab on TALL");
        await walk(clientA, phone, "bottom", "top");
    });

    it("should line the radio rows up from the bottom", async () => {
        const clientA = getClient("clientA");
        await openOverflowProfile(clientA, 32);
        const radio = () => splitRadioPane(clientA);

        await waitForOverflowTab(clientA, radio, "top", true, "No top tab on the radio pane");
        await walk(clientA, radio, "top", "bottom", true);
    });

    it("should keep a few radio stations at the bottom of the radio pane", async () => {
        const clientA = getClient("clientA");
        await openOverflowProfile(clientA, 3);
        const radio = () => splitRadioPane(clientA);

        const {insets, gap} = await geometry(clientA, radio, "y", true);
        const lowest = Math.min(...insets);
        if (Math.abs(lowest - gap) > 1.5) {
            throw new Error(`The lowest station sits ${lowest}px above the bottom, not ${gap}px`);
        }
        await expectOverflowTabStays(radio, "top", false, 500, "above a few stations");
        await expectOverflowTabStays(radio, "bottom", false, 500, "below a few stations");
    });

    it("should line the columns up with a zoomed app", async () => {
        const clientA = getClient("clientA");
        await openOverflowProfile(clientA, 3);
        const phone = () => splitPhonePane(clientA);
        await clientA.execute(() => {
            document.documentElement.style.zoom = "1.25";
        });

        await waitForOverflowTab(clientA, phone, "right", true, "No right tab on WIDE");
        await walk(clientA, phone, "right", "left");
    });

    // The app's own zoom is the webview's, which rounds scroll offsets differently from CSS zoom.
    for (const [direction, change] of [
        ["out", "decrease"],
        ["in", "increase"],
    ] as const) {
        it(`should line the columns up zoomed ${direction} with the zoom hotkeys`, async () => {
            const clientA = getClient("clientA");
            await openOverflowProfile(clientA, 3);
            const phone = () => splitPhonePane(clientA);

            for (let steps = 1; steps <= 3; steps++) {
                await changeZoom(clientA, change);
                await waitForOverflowTab(clientA, phone, "right", true, `No right tab, ${steps}`);
                await walk(clientA, phone, "right", "left");
                await rewind(clientA, phone, "left");
            }
            await changeZoom(clientA, "reset");
        });

        it(`should line the rows up zoomed ${direction} with the zoom hotkeys`, async () => {
            const clientA = getClient("clientA");
            await openOverflowProfile(clientA, 32);
            await selectTab(clientA, "TALL", firstKey("T"));
            const phone = () => splitPhonePane(clientA);
            const radio = () => splitRadioPane(clientA);

            for (let steps = 1; steps <= 3; steps++) {
                await changeZoom(clientA, change);
                await waitForOverflowTab(clientA, phone, "bottom", true, `No bottom tab, ${steps}`);
                await walk(clientA, phone, "bottom", "top");
                await rewind(clientA, phone, "top");
                await waitForOverflowTab(clientA, radio, "top", true, `No top tab, ${steps}`);
                await walk(clientA, radio, "top", "bottom", true);
                await rewind(clientA, radio, "bottom", true);
            }
            await changeZoom(clientA, "reset");
        });
    }
});
