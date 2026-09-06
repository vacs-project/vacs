import {afterEach, beforeEach, describe, expect, it, vi} from "vitest";
import {cleanup, render, waitFor} from "@testing-library/preact";

// jsdom has no ResizeObserver; PlaybackList's List component observes its container.
class ResizeObserverStub {
    observe() {}
    unobserve() {}
    disconnect() {}
}

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import PlaybackPage from "../../src/pages/PlaybackPage.tsx";
import {useCapabilitiesStore} from "../../src/stores/capabilities-store.ts";
import {usePlaybackStore} from "../../src/stores/playback-store.ts";
import {useRadioStore} from "../../src/stores/radio-store.ts";
import {useSettingsStore} from "../../src/stores/settings-store.ts";
import {INSTANCE_ID} from "../../src/transport/store-sync.ts";
import {RadioConfigWithLabels} from "../../src/types/transmit.ts";

const RADIO_CONFIG: RadioConfigWithLabels = {
    integration: "TrackAudio",
    audioForVatsim: null,
    trackAudio: {endpoint: "ws://localhost:49080"},
};

function makeAvailable() {
    useCapabilitiesStore.setState({playback: true});
    useSettingsStore.setState({radioConfig: RADIO_CONFIG, playbackEnabled: true});
    useRadioStore.setState({radioState: {state: "Connected"}});
}

beforeEach(() => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
});

afterEach(() => {
    useCapabilitiesStore.setState({playback: false});
    useSettingsStore.setState({radioConfig: undefined, playbackEnabled: false});
    useRadioStore.setState({radioState: undefined});
    usePlaybackStore.setState({
        selected: 0,
        status: undefined,
        playbackDevice: "Output",
        openInstanceIds: [],
    });
    vi.clearAllMocks();
    vi.unstubAllGlobals();
    cleanup();
});

describe("PlaybackPage", () => {
    it("does not stop playback on unmount when the say-again key owns it", async () => {
        makeAvailable();
        usePlaybackStore.setState({
            status: {id: 1, status: "playing", continuously: false, sayAgain: true, progress: 0},
        });
        const {unmount} = render(<PlaybackPage />);
        await waitFor(() =>
            expect(usePlaybackStore.getState().openInstanceIds).toContain(INSTANCE_ID),
        );

        unmount();

        expect(invoke).not.toHaveBeenCalledWith("playback_stop", expect.anything());
        expect(usePlaybackStore.getState().openInstanceIds).toEqual([]);
    });

    it("stops playback on unmount when the page itself owns it", async () => {
        makeAvailable();
        usePlaybackStore.setState({
            status: {id: 1, status: "playing", continuously: false, sayAgain: false, progress: 0},
        });
        const {unmount} = render(<PlaybackPage />);
        await waitFor(() =>
            expect(usePlaybackStore.getState().openInstanceIds).toContain(INSTANCE_ID),
        );

        unmount();

        await waitFor(() => expect(invoke).toHaveBeenCalledWith("playback_stop", undefined));
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "playback_stop")).toHaveLength(1);
    });

    it("does not stop playback on unmount when another instance is still open", async () => {
        makeAvailable();
        usePlaybackStore.setState({
            status: {id: 1, status: "playing", continuously: false, sayAgain: false, progress: 0},
            openInstanceIds: ["other"],
        });
        const {unmount} = render(<PlaybackPage />);
        await waitFor(() =>
            expect(usePlaybackStore.getState().openInstanceIds).toEqual(["other", INSTANCE_ID]),
        );

        unmount();

        expect(usePlaybackStore.getState().openInstanceIds).toEqual(["other"]);
        expect(invoke).not.toHaveBeenCalledWith("playback_stop", expect.anything());
    });
});
