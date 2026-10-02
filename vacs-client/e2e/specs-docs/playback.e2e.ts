import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnectAs, resetMockState} from "../helpers/auth.ts";
import {click, getClient, mockCommand} from "../helpers/browser.ts";
import {annotate, clearAnnotations} from "../helpers/annotate.ts";
import {
    applyFixtures,
    applyRadioPlaybackMocks,
    capturePhase,
    CID_A,
    CLIPS,
    emitEvent,
    POSITION_A,
    subPage,
} from "../helpers/docs.ts";
import {type GifFrame, writeGif} from "../helpers/gif.ts";
import {captureElement, captureFrame, captureWindow} from "../helpers/screenshot.ts";

const SAY_AGAIN_BUTTON = '//button[.//p[contains(., "SAY")]]';
const PLAYBACK_BUTTON = '//button[.//p[contains(., "PLAY")]]';
/** The eight playback control buttons, two rows filled column by column. */
const CONTROLS = '//div[contains(@class, "grid-rows-2") and contains(@class, "gap-y-3")]';

/**
 * The playback controls in document order. The grid fills column by column,
 * so the top row is play, continuous, previous, next and the bottom row
 * headset, stop, rewind, fast forward.
 */
const CONTROL = {
    play: `(${CONTROLS}/button)[1]`,
    headset: `(${CONTROLS}/button)[2]`,
    continuous: `(${CONTROLS}/button)[3]`,
    stop: `(${CONTROLS}/button)[4]`,
    previous: `(${CONTROLS}/button)[5]`,
    rewind: `(${CONTROLS}/button)[6]`,
    next: `(${CONTROLS}/button)[7]`,
    forward: `(${CONTROLS}/button)[8]`,
};
const EXPORT_BUTTON = '//button[./p[text()="Export"]]';
const DELETE_BUTTON = '//button[./p[text()="Delete"]]';
const DELETE_ALL_BUTTON = '//button[./p[contains(., "All")]]';

async function openPlaybackPage(browser: WebdriverIO.Browser): Promise<void> {
    await click(browser, browser.$(PLAYBACK_BUTTON));
    await subPage(browser, "Playback").waitForDisplayed();
    await browser
        .$(`//*[contains(text(), "${CLIPS[CLIPS.length - 1].callsigns[0]}")]`)
        .waitForDisplayed();
}

describe("Documentation screenshots: playback", function () {
    this.timeout(120_000);

    beforeEach(async () => {
        await resetMockState();
        await restartApps();
    });

    it("captures the SAY AGAIN function key", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");
        await applyRadioPlaybackMocks("clientA");

        const sayAgain = clientA.$(SAY_AGAIN_BUTTON);
        await clientA.waitUntil(async () => await sayAgain.isEnabled(), {
            timeoutMsg: "SAY AGAIN did not become available",
        });

        await captureElement(
            clientA,
            clientA.$(`${SAY_AGAIN_BUTTON}/..`),
            "playback/say-again-button.png",
        );
    });

    it("captures the Playback page and its controls", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");
        await applyRadioPlaybackMocks("clientA");

        await openPlaybackPage(clientA);
        await captureWindow(clientA, "playback/playback_overview.png");

        // Numbered as in the page's table of controls.
        await annotate(clientA, [
            {target: CONTROL.play, badge: 1},
            {target: CONTROL.continuous, badge: 2},
            {target: CONTROL.previous, badge: 3},
            {target: CONTROL.next, badge: 4},
            {target: CONTROL.headset, badge: 5},
            {target: CONTROL.stop, badge: 6},
            {target: CONTROL.rewind, badge: 7},
            {target: CONTROL.forward, badge: 8},
            {target: EXPORT_BUTTON, badge: 9},
            {target: DELETE_BUTTON, badge: 10},
            {target: DELETE_ALL_BUTTON, badge: 11},
        ]);
        await captureWindow(clientA, "playback/playback_controls.png");
        await clearAnnotations(clientA);
    });

    it("records the playback controls in use", async () => {
        const clientA = getClient("clientA");
        await loginAndConnectAs(clientA, CID_A, POSITION_A);
        await applyFixtures(clientA, "clientA");
        await applyRadioPlaybackMocks("clientA");

        // The mock audio backend plays nothing, so the commands are mocked and
        // the progress the backend would report while a clip plays is emitted.
        for (const command of [
            "playback_start",
            "playback_pause",
            "playback_continue",
            "playback_stop",
            "playback_seek",
        ]) {
            await mockCommand("clientA", command, {resolve: null});
        }

        await openPlaybackPage(clientA);

        const frames: GifFrame[] = [];
        const hold = async (delay: number) => {
            frames.push({png: await captureFrame(clientA), delay});
        };
        const press = async (control: string) => {
            await click(clientA, clientA.$(control));
        };
        const progress = async (value: number, delay = 900) => {
            await emitEvent("clientA", "playback:progress", value);
            await hold(delay);
        };
        const playLit = async () =>
            ((await clientA.$(CONTROL.play).getAttribute("class")) ?? "").includes("bg-blue-700");

        await hold(1500);

        await press(CONTROL.play);
        await progress(0.25);
        await progress(0.5);

        await press(CONTROL.play);
        for (let cycle = 0; cycle < 2; cycle++) {
            frames.push({png: await capturePhase(clientA, playLit, true), delay: 500});
            frames.push({png: await capturePhase(clientA, playLit, false), delay: 500});
        }

        await press(CONTROL.play);
        await progress(0.75);

        await press(CONTROL.stop);
        await hold(1200);

        // Continuous play needs a newer clip to continue with, so it starts
        // from an older one.
        await press('//p[starts-with(text(), "AUA25")]');
        await hold(900);
        await press(CONTROL.continuous);
        await progress(0.4);
        await press(CONTROL.next);
        await progress(0.3);
        await press(CONTROL.previous);
        await progress(0.2);
        await press(CONTROL.forward);
        await progress(0.5);
        await press(CONTROL.rewind);
        await progress(0.35);
        await press(CONTROL.headset);
        await progress(0.6);

        await press(CONTROL.stop);
        await hold(1500);

        writeGif("playback/playback_controls.gif", frames);
    });
});
