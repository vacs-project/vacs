import path from "node:path";
import {fileURLToPath} from "node:url";
import {
    clearPersistedClientSettings,
    readPersistedClientSettings,
    restartApps,
} from "../helpers/app-control.ts";
import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {
    click,
    doubleClick,
    dragHorizontally,
    getClient,
    pageButton,
    pageCycleButton,
    pageCycleCell,
    pageTab,
    selectOption,
    splitPhonePane,
    splitResizeHandle,
} from "../helpers/browser.ts";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

// A CID without a datafeed controller: it stays positionless, so the server
// never pushes a profile of its own that would replace the test profile.
const CID_A = "10000004";

// Profiles with the split and the cycle view, loaded through the test profile
// mechanism. The dataset the harness serves has no split-view profile, and a
// server-pushed profile is the only other way to get one.
const SPLIT_PROFILE = path.resolve(__dirname, "..", "fixtures", "profile-split.json");
const CYCLE_PROFILE = path.resolve(__dirname, "..", "fixtures", "profile-cycle.json");
const SPLIT_PROFILE_ID = "E2E_SPLIT";

// A direct access key of each fixture, which marks the phone pane as rendered.
const SPLIT_KEY = '//button[.//p[@title="SPLIT"] and .//p[@title="A1"]]';
const CYCLE_KEY = '//button[.//p[@title="CYCLE"] and .//p[@title="A1"]]';

// The radio pane without a TrackAudio connection. Which of the two messages it
// shows depends on how far the connection attempt got: the client reports
// Disconnected while it is still retrying and Error once an attempt failed
// outright, and both render the same placeholder with the Retry link.
const RADIO_PLACEHOLDER =
    '//p[text()="No TrackAudio radio connection." or text()="TrackAudio radio connection failed."]';
const RADIO_RETRY = '//p[text()="Retry"]';

const SETTINGS_BUTTON = '//button[.//img[@alt="Settings"]]';

async function openSettings(browser: WebdriverIO.Browser): Promise<void> {
    const settingsButton = await browser.$(SETTINGS_BUTTON);
    await settingsButton.waitForDisplayed();
    await click(browser, settingsButton);
}

/** The settings button toggles, so the same click closes the menu again. */
async function closeSettings(browser: WebdriverIO.Browser): Promise<void> {
    await click(browser, browser.$(SETTINGS_BUTTON));
    await browser.$('//p[text()="Settings"]').waitForDisplayed({reverse: true});
}

/**
 * Switches the radio integration in Settings > Transmit. The backend refuses
 * the command outright on a platform without a keybind listener (Linux X11 and
 * Wayland-with-portal both have one), and the frontend then rolls the select
 * back, which is what the wait below catches.
 */
async function setRadioIntegration(
    browser: WebdriverIO.Browser,
    integration: "None" | "TrackAudio" | "AudioForVatsim",
): Promise<void> {
    await openSettings(browser);
    const transmitButton = await browser.$('//button[./p[text()="Transmit"]]');
    await transmitButton.waitForDisplayed();
    await click(browser, transmitButton);

    const select = await browser.$('//select[@name="radio-integration"]');
    await select.waitForDisplayed();
    await selectOption(browser, 'select[name="radio-integration"]', integration);
    await browser.waitUntil(async () => (await select.getValue()) === integration, {
        timeoutMsg: `Radio integration did not stay on ${integration} (the backend rejected it)`,
    });

    await closeSettings(browser);
}

async function loadTestProfile(browser: WebdriverIO.Browser, file: string): Promise<void> {
    const result = await browser.execute(async (profilePath: string) => {
        try {
            await window.__TAURI_INTERNALS__.invoke("app_load_test_profile", {path: profilePath});
            return {ok: true as const};
        } catch (e) {
            return {ok: false as const, error: String(e)};
        }
    }, file);

    if (!result.ok) {
        throw new Error(`app_load_test_profile failed for ${file}: ${result.error}`);
    }
}

/** The width the client persisted for the split fixture, if any. */
function persistedSplitWidth(): number | undefined {
    const settings = readPersistedClientSettings();
    const match = settings?.match(new RegExp(`${SPLIT_PROFILE_ID}"?\\s*=\\s*(\\d+)`));
    return match === null || match === undefined ? undefined : Number(match[1]);
}

describe("Split view profiles", function () {
    // Each test walks the settings, the profile load and several page
    // switches; the default 60s ceiling leaves no room for the app boot on a
    // cold machine. Set on the suite: a per-test timeout is ignored.
    this.timeout(120_000);

    beforeEach(async () => {
        await resetMockState();
        await restartApps();
        // After the restart, so no dying instance of the previous generation
        // can write its entry back into the file the width assertions read.
        clearPersistedClientSettings();

        await loginAndConnect(getClient("clientA"), CID_A);
    });

    it("should show the phone and radio tabs and open the mixed page for a split profile", async () => {
        const clientA = getClient("clientA");
        await setRadioIntegration(clientA, "TrackAudio");
        await loadTestProfile(clientA, SPLIT_PROFILE);

        // The tabs replace the Radio and Phone buttons of the bottom row.
        await pageTab(clientA, "Phone").waitForDisplayed();
        await pageTab(clientA, "Radio").waitForDisplayed();
        if (await pageButton(clientA, "Radio").isExisting()) {
            throw new Error("The ordinary Radio button is still rendered next to the page tabs");
        }

        // A split profile opens on the mixed page: both panes, separated by
        // the drag handle.
        await splitResizeHandle(clientA).waitForExist();
        await clientA.$(RADIO_PLACEHOLDER).waitForDisplayed();
        await clientA.$(SPLIT_KEY).waitForDisplayed();

        // The Phone tab leaves the phone page alone in the main area.
        await click(clientA, pageTab(clientA, "Phone"));
        await splitResizeHandle(clientA).waitForExist({reverse: true});
        await clientA.$(RADIO_PLACEHOLDER).waitForDisplayed({reverse: true});
        await clientA.$(SPLIT_KEY).waitForDisplayed();

        // The Radio tab goes back to the mixed page rather than to a radio
        // page of its own.
        await click(clientA, pageTab(clientA, "Radio"));
        await splitResizeHandle(clientA).waitForExist();
        await clientA.$(RADIO_PLACEHOLDER).waitForDisplayed();
        await clientA.$(SPLIT_KEY).waitForDisplayed();
    });

    it("should cycle radio, phone and mixed pages for a cycle profile", async () => {
        const clientA = getClient("clientA");
        await setRadioIntegration(clientA, "TrackAudio");
        await loadTestProfile(clientA, CYCLE_PROFILE);

        const cycleButton = pageCycleButton(clientA);
        await cycleButton.waitForDisplayed();
        if (await pageTab(clientA, "Phone").isExisting()) {
            throw new Error("A cycle profile rendered the split profile's page tabs");
        }

        const activeCell = async (letter: "R" | "P" | "M") =>
            ((await pageCycleCell(clientA, letter).getAttribute("class")) ?? "").includes(
                "bg-gray-400",
            );

        // A cycle profile also opens on the mixed page, so the first step of
        // the cycle is the radio page.
        await clientA.waitUntil(() => activeCell("M"), {
            timeoutMsg: "The cycle profile did not open on the mixed page",
        });
        await splitResizeHandle(clientA).waitForExist();

        await click(clientA, cycleButton);
        await clientA.waitUntil(() => activeCell("R"), {
            timeoutMsg: "The Page button did not step to the radio page",
        });
        await clientA.$(RADIO_PLACEHOLDER).waitForDisplayed();
        await clientA.$(CYCLE_KEY).waitForDisplayed({reverse: true});
        await splitResizeHandle(clientA).waitForExist({reverse: true});

        await click(clientA, cycleButton);
        await clientA.waitUntil(() => activeCell("P"), {
            timeoutMsg: "The Page button did not step to the phone page",
        });
        await clientA.$(CYCLE_KEY).waitForDisplayed();
        await clientA.$(RADIO_PLACEHOLDER).waitForDisplayed({reverse: true});

        await click(clientA, cycleButton);
        await clientA.waitUntil(() => activeCell("M"), {
            timeoutMsg: "The Page button did not step back to the mixed page",
        });
        await clientA.$(CYCLE_KEY).waitForDisplayed();
        await clientA.$(RADIO_PLACEHOLDER).waitForDisplayed();
    });

    it("should shrink the direct access keys for a split view profile", async () => {
        const clientA = getClient("clientA");
        await loadTestProfile(clientA, SPLIT_PROFILE);

        // Without the TrackAudio integration the profile renders as an
        // ordinary tabbed one, with full width keys.
        const key = await clientA.$(SPLIT_KEY);
        await key.waitForDisplayed();
        await clientA.waitUntil(
            async () => ((await key.getAttribute("style")) ?? "").includes("6.25rem"),
            {timeoutMsg: "A direct access key was not 6.25rem wide without the split view"},
        );

        // The keys shrink with the split view itself, not with the mixed page:
        // the narrower key is what makes the phone pane fit next to the radio.
        await setRadioIntegration(clientA, "TrackAudio");
        await clientA.waitUntil(
            async () => ((await key.getAttribute("style")) ?? "").includes("5.5rem"),
            {timeoutMsg: "A direct access key did not shrink to 5.5rem in the split view"},
        );
    });

    it("should keep the ordinary page controls without the TrackAudio integration", async () => {
        const clientA = getClient("clientA");
        await loadTestProfile(clientA, SPLIT_PROFILE);

        // No integration is the state the client boots in.
        await pageButton(clientA, "Radio").waitForDisplayed();
        await pageButton(clientA, "Phone").waitForDisplayed();
        if (await pageTab(clientA, "Radio").isExisting()) {
            throw new Error("A split profile showed the page tabs without a radio integration");
        }

        await loadTestProfile(clientA, CYCLE_PROFILE);
        await clientA.$(CYCLE_KEY).waitForDisplayed();
        await pageButton(clientA, "Radio").waitForDisplayed();
        if (await pageCycleButton(clientA).isExisting()) {
            throw new Error("A cycle profile showed the Page button without a radio integration");
        }

        // Audio for Vatsim is only offered where the platform can emit
        // keybinds (not on Wayland), and the backend gates the integration on
        // the same capability.
        const afvOption = await clientA.$(
            '//select[@name="radio-integration"]/option[@value="AudioForVatsim"]',
        );
        await openSettings(clientA);
        const transmitButton = await clientA.$('//button[./p[text()="Transmit"]]');
        await transmitButton.waitForDisplayed();
        await click(clientA, transmitButton);
        await clientA.$('//select[@name="radio-integration"]').waitForDisplayed();
        const afvAvailable = await afvOption.isExisting();
        await closeSettings(clientA);

        if (afvAvailable) {
            await setRadioIntegration(clientA, "AudioForVatsim");
            await loadTestProfile(clientA, SPLIT_PROFILE);
            await pageButton(clientA, "Radio").waitForDisplayed();
            if (await pageTab(clientA, "Radio").isExisting()) {
                throw new Error("A split profile showed the page tabs with Audio for Vatsim");
            }
        }
    });

    it("should offer a retry on the radio pane without a TrackAudio connection", async () => {
        const clientA = getClient("clientA");
        await setRadioIntegration(clientA, "TrackAudio");
        await loadTestProfile(clientA, SPLIT_PROFILE);

        await clientA.$(RADIO_PLACEHOLDER).waitForDisplayed();
        const retry = await clientA.$(RADIO_RETRY);
        await retry.waitForDisplayed();

        // Nothing answers on the TrackAudio port, so a retry leaves the pane
        // where it was instead of replacing it with the station list.
        await click(clientA, retry);
        await clientA.$(RADIO_PLACEHOLDER).waitForDisplayed();
        await clientA.$(SPLIT_KEY).waitForDisplayed();
    });

    it("should resize the phone pane, persist the width and reset it on a double click", async () => {
        const clientA = getClient("clientA");
        await setRadioIntegration(clientA, "TrackAudio");
        await loadTestProfile(clientA, SPLIT_PROFILE);

        const pane = splitPhonePane(clientA);
        await pane.waitForDisplayed();
        const defaultWidth = (await pane.getSize()).width;

        // Left grows the phone pane: its width is measured from its right
        // border to the pointer.
        const grownBy = 120;
        await dragHorizontally(clientA, splitResizeHandle(clientA), -grownBy, async () => {
            await clientA.waitUntil(
                async () => (await pane.getSize()).width > defaultWidth + grownBy / 2,
                {timeoutMsg: "The phone pane did not grow while the handle was dragged"},
            );
        });

        const draggedWidth = (await pane.getSize()).width;

        // The release persists the width under the profile id, which is what
        // a later session gets handed back in its session info.
        await clientA.waitUntil(async () => persistedSplitWidth() !== undefined, {
            timeoutMsg: "The dragged width was not persisted for the profile",
        });
        const persisted = persistedSplitWidth() ?? 0;
        if (Math.abs(persisted - draggedWidth) > 2) {
            throw new Error(
                `Persisted width ${persisted} does not match the rendered ${draggedWidth}`,
            );
        }

        // A double click on the handle drops back to the default layout and
        // clears the stored width again.
        await doubleClick(clientA, splitResizeHandle(clientA));
        await clientA.waitUntil(async () => (await pane.getSize()).width === defaultWidth, {
            timeoutMsg: "The phone pane did not return to its default width",
        });
        await clientA.waitUntil(async () => persistedSplitWidth() === undefined, {
            timeoutMsg: "The persisted width was not cleared by the double click",
        });
    });
});
