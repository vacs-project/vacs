import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnectAs, resetMockState} from "../helpers/auth.ts";
import {click, frequencyObjects, getClient, mockCommand, pageButton} from "../helpers/browser.ts";
import {
    applyFixtures,
    applyTrackAudioMocks,
    capturePhase,
    CID_A,
    emitEvent,
    openRadioPage,
    POSITION_A,
    radioStation,
    type RadioStationFixture,
} from "../helpers/docs.ts";
import {type GifFrame, writeGif} from "../helpers/gif.ts";
import {captureElement, captureFrame, captureWindow} from "../helpers/screenshot.ts";

// The radio stack of the manual's earlier overview image: a center position's
// usual frequencies, none of them selected yet.
const OVERVIEW_STATIONS = [
    radioStation("LOWL_APP", 125_685_000),
    radioStation("LOWS_APP", 123_725_000),
    radioStation("LOWW_APP", 134_675_000),
    radioStation("LOXZ_APP", 129_480_000),
    radioStation("LOVV_S_CTR", 122_865_000),
    radioStation("LOVV_U_CTR", 131_350_000),
    radioStation("LOWG_APP", 119_300_000),
    radioStation("LOWI_APP", 128_975_000),
    radioStation("LOWK_APP", 123_325_000),
    radioStation("GUARD", 121_500_000),
    radioStation("UNICOM", 122_800_000),
    radioStation("LOVV_CTR", 132_600_000),
    radioStation("LOVV_I_CTR", 124_400_000),
    radioStation("LOVV_L_CTR", 129_200_000),
];

const LOVV_CTR = 132_600_000;
const LOVV_I_CTR = 124_400_000;

/** The station of the frequency object crops. */
const SINGLE = radioStation("LOVV_CTR", LOVV_CTR);

const CPL_BUTTON = '//button[./p[text()="CPL"]]';
const FAST_CPL_BUTTON = '//button[./p[contains(., "FAST")]]';
const NO_CONNECTION = '//p[text()="No TrackAudio radio connection."]';

/** The left half of a frequency object, which selects it in couple mode. */
function frequencyObjectLabel(callsign: string): string {
    return `//button[.//p[text()="${callsign}"]]`;
}

/**
 * Sends a station update as TrackAudio would after a state change. The radio
 * page applies it to its station map in place, which does not re-render the
 * page, so the radio state is sent again afterwards to make the update show.
 */
async function updateStation(station: RadioStationFixture): Promise<void> {
    await emitEvent("clientA", "radio:station-updated", station);
    await emitEvent("clientA", "radio:state", {state: "Connected"});
}

async function connect(browser: WebdriverIO.Browser): Promise<void> {
    await loginAndConnectAs(browser, CID_A, POSITION_A);
    await applyFixtures(browser, "clientA");
}

/** Opens the Radio page on the given stations and waits for all of them. */
async function openStations(
    browser: WebdriverIO.Browser,
    stations: RadioStationFixture[],
    options: {cplMode?: "Original" | "Fast"} = {},
): Promise<void> {
    await applyTrackAudioMocks("clientA", "Connected", stations, options);
    // Every toggle on a frequency object invokes these; TrackAudio's answer is
    // the station update the test emits.
    await mockCommand("clientA", "radio_set_station_state", {resolve: null});
    await mockCommand("clientA", "radio_fast_couple", {resolve: null});
    await openRadioPage(browser, browser.$(frequencyObjectLabel(stations[0].callsign)));
    await browser.waitUntil(
        async () => (await frequencyObjects(browser).getElements()).length === stations.length,
        {timeoutMsg: "The Radio page did not show every mocked station"},
    );
}

describe("Documentation screenshots: radio", function () {
    this.timeout(120_000);

    beforeEach(async () => {
        await resetMockState();
        await restartApps();
    });

    it("captures the Radio page", async () => {
        const clientA = getClient("clientA");
        await connect(clientA);
        await openStations(clientA, OVERVIEW_STATIONS);

        await captureWindow(clientA, "radio/radio_overview.png");
    });

    it("captures a frequency object in each of its states", async () => {
        const clientA = getClient("clientA");
        await connect(clientA);
        await openStations(clientA, [SINGLE]);

        const capture = async (name: string) =>
            captureElement(clientA, frequencyObjects(clientA)[0], `radio/${name}.png`);

        await capture("radio_freqobj");

        await updateStation({...SINGLE, rx: true});
        await capture("radio_freqobj_rx");

        await updateStation({...SINGLE, rx: true, tx: true});
        await capture("radio_freqobj_tx");

        await emitEvent("clientA", "radio:state", {state: "RxActive", data: [LOVV_CTR]});
        await capture("radio_freqobj_rx_active");

        await emitEvent("clientA", "radio:state", {state: "TxActive"});
        await capture("radio_freqobj_tx_active");

        await updateStation({...SINGLE, rx: true, tx: true, headset: false});
        await capture("radio_freqobj_speaker");

        await updateStation({...SINGLE, rx: true, tx: true, xca: true});
        await capture("radio_freqobj_cross_coupled");
    });

    it("records cross coupling in the original couple mode", async () => {
        const clientA = getClient("clientA");
        await connect(clientA);
        const stations = OVERVIEW_STATIONS.map(station =>
            station.frequency === LOVV_CTR ? {...station, rx: true, tx: true} : station,
        );
        await openStations(clientA, stations);

        const ctr = stations.find(station => station.frequency === LOVV_CTR)!;
        const sector = stations.find(station => station.frequency === LOVV_I_CTR)!;
        const frames: GifFrame[] = [];
        const cpl = clientA.$(CPL_BUTTON);
        const cplLit = async () =>
            ((await cpl.getAttribute("class")) ?? "").includes("bg-blue-700");
        const blinkCycle = async () => {
            frames.push({png: await capturePhase(clientA, cplLit, true), delay: 500});
            frames.push({png: await capturePhase(clientA, cplLit, false), delay: 500});
        };

        frames.push({png: await captureFrame(clientA), delay: 1200});

        // Couple mode blinks CPL; each selected frequency couples, which
        // turns its Rx and Tx on.
        await click(clientA, cpl);
        await blinkCycle();
        await click(clientA, clientA.$(frequencyObjectLabel(ctr.callsign)));
        await updateStation({...ctr, xca: true});
        await blinkCycle();
        await click(clientA, clientA.$(frequencyObjectLabel(sector.callsign)));
        await updateStation({...sector, rx: true, tx: true, xca: true});
        await blinkCycle();

        await click(clientA, cpl);
        await clientA.waitUntil(async () => !(await cplLit()), {
            timeoutMsg: "CPL did not leave couple mode",
        });
        frames.push({png: await captureFrame(clientA), delay: 2000});

        writeGif("radio/radio_cross_couple_original.gif", frames);
    });

    it("records cross coupling in the fast couple mode", async () => {
        const clientA = getClient("clientA");
        await connect(clientA);
        const stations = OVERVIEW_STATIONS.map(station =>
            station.frequency === LOVV_CTR || station.frequency === LOVV_I_CTR
                ? {...station, rx: true, tx: true}
                : station,
        );
        await openStations(clientA, stations, {cplMode: "Fast"});
        await clientA.$(FAST_CPL_BUTTON).waitForDisplayed();

        const ctr = stations.find(station => station.frequency === LOVV_CTR)!;
        const sector = stations.find(station => station.frequency === LOVV_I_CTR)!;
        const frames: GifFrame[] = [];
        frames.push({png: await captureFrame(clientA), delay: 1500});

        // FAST CPL couples every frequency with Tx on at once.
        await click(clientA, clientA.$(FAST_CPL_BUTTON));
        await updateStation({...ctr, xca: true});
        await updateStation({...sector, xca: true});
        frames.push({png: await captureFrame(clientA), delay: 2000});

        // Turning Tx off uncouples that one frequency.
        await click(
            clientA,
            clientA.$(`${frequencyObjectLabel(sector.callsign)}/../../div[3]/button`),
        );
        await updateStation({...sector, tx: false, xca: false});
        frames.push({png: await captureFrame(clientA), delay: 2000});

        writeGif("radio/radio_cross_couple_fast.gif", frames);
    });

    it("captures the Radio page without a TrackAudio connection", async () => {
        const clientA = getClient("clientA");
        await connect(clientA);
        await applyTrackAudioMocks("clientA", "Disconnected");

        await openRadioPage(clientA, clientA.$(NO_CONNECTION));
        await clientA.$('//p[text()="Retry"]').waitForDisplayed();

        await captureWindow(clientA, "radio/radio_no_connection.png");
    });

    it("captures the radio button in its error state", async () => {
        const clientA = getClient("clientA");
        await connect(clientA);

        const radioButton = pageButton(clientA, "Radio");
        await radioButton.waitForDisplayed();

        await emitEvent("clientA", "radio:state", {state: "Error"});
        await clientA.waitUntil(
            async () => ((await radioButton.getAttribute("class")) ?? "").includes("red"),
            {timeoutMsg: "The radio button did not turn red"},
        );

        await captureElement(clientA, radioButton, "radio/radio_button_error.png", {padding: 8});
    });
});
