import {afterEach, beforeEach, describe, expect, it, vi} from "vitest";
import {act, cleanup, render, waitFor} from "@testing-library/preact";

// jsdom has no ResizeObserver; PlaybackList's List component observes its container
// and renders no rows at all unless it is told a height.
class ResizeObserverStub {
    constructor(private callback: ResizeObserverCallback) {}
    observe(target: Element) {
        this.callback(
            [{target, contentRect: {height: 600}} as unknown as ResizeObserverEntry],
            this,
        );
    }
    unobserve() {}
    disconnect() {}
}

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import PlaybackPage from "../../src/pages/PlaybackPage.tsx";
import {useCapabilitiesStore} from "../../src/stores/capabilities-store.ts";
import {usePlaybackStore} from "../../src/stores/playback-store.ts";
import {useRadioStore} from "../../src/stores/radio-store.ts";
import {useSettingsStore} from "../../src/stores/settings-store.ts";
import {INSTANCE_ID} from "../../src/transport/store-sync.ts";
import {ClipMeta} from "../../src/types/playback.ts";
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

function makeClip(overrides: Partial<ClipMeta> = {}): ClipMeta {
    return {
        id: 1,
        path: "/tmp/clip.wav",
        callsigns: ["VIE_TWR"],
        frequency: 118_300_000,
        startedAt: {secs_since_epoch: 0, nanos_since_epoch: 0},
        endedAt: {secs_since_epoch: 1, nanos_since_epoch: 0},
        durationMs: 1000,
        ...overrides,
    };
}

const OLD_CLIP = makeClip({
    id: 1,
    callsigns: ["OLD_TWR"],
    startedAt: {secs_since_epoch: 10, nanos_since_epoch: 0},
});
const MID_CLIP = makeClip({
    id: 2,
    callsigns: ["MID_TWR"],
    startedAt: {secs_since_epoch: 20, nanos_since_epoch: 0},
});
const NEW_CLIP = makeClip({
    id: 3,
    callsigns: ["NEW_TWR"],
    startedAt: {secs_since_epoch: 30, nanos_since_epoch: 0},
});

const OLD_ROW = "OLD_TWR\\118.300";
const MID_ROW = "MID_TWR\\118.300";
const NEW_ROW = "NEW_TWR\\118.300";

function invokedCommands(): string[] {
    return invoke.mock.calls.map(([cmd]) => cmd);
}

function listedClips(): string[] {
    return Array.from(document.querySelectorAll("p.truncate"))
        .map(cell => cell.textContent ?? "")
        .filter(text => text.length > 0);
}

function selectedRow(): string | undefined {
    return listedClips()[usePlaybackStore.getState().selected];
}

async function renderWithClips(clips: ClipMeta[]) {
    invoke.mockImplementation((cmd: string) =>
        cmd === "playback_list" ? Promise.resolve(clips) : Promise.resolve(undefined),
    );
    render(<PlaybackPage />);
    await waitFor(() => expect(listedClips()).toHaveLength(clips.length));
}

function emitClipsModified(recorded: ClipMeta, evicted: ClipMeta[] = []) {
    const call = listen.mock.calls.find(([event]) => event === "playback:clips-modified");
    if (call === undefined) throw new Error("playback:clips-modified was never registered");
    void act(() => call[1]({payload: {recorded, evicted}}));
}

beforeEach(() => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
});

afterEach(() => {
    // Unmount first: the teardown effect can still stop playback, and that call
    // must not leak into the next test's invoke history.
    cleanup();
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

        expect(invokedCommands()).not.toContain("playback_stop");
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
        expect(invokedCommands()).not.toContain("playback_stop");
    });

    it("keeps the selection on the same clip when a recording arrives while idle", async () => {
        makeAvailable();
        usePlaybackStore.setState({selected: 1});
        await renderWithClips([MID_CLIP, OLD_CLIP]);
        expect(selectedRow()).toBe(OLD_ROW);

        emitClipsModified(NEW_CLIP);

        await waitFor(() => expect(listedClips()).toEqual([NEW_ROW, MID_ROW, OLD_ROW]));
        expect(usePlaybackStore.getState().selected).toBe(2);
        expect(selectedRow()).toBe(OLD_ROW);
    });

    it("keeps the selection on the playing clip when a recording arrives", async () => {
        makeAvailable();
        usePlaybackStore.setState({
            selected: 1,
            status: {
                id: OLD_CLIP.id,
                status: "playing",
                continuously: false,
                sayAgain: false,
                progress: 0.5,
            },
        });
        await renderWithClips([MID_CLIP, OLD_CLIP]);

        emitClipsModified(NEW_CLIP);

        await waitFor(() => expect(listedClips()).toEqual([NEW_ROW, MID_ROW, OLD_ROW]));
        expect(usePlaybackStore.getState().selected).toBe(2);
        expect(selectedRow()).toBe(OLD_ROW);
        expect(invokedCommands()).not.toContain("playback_stop");
    });

    it("leaves the selection alone when the selected oldest clip is evicted", async () => {
        makeAvailable();
        usePlaybackStore.setState({selected: 1});
        await renderWithClips([MID_CLIP, OLD_CLIP]);

        emitClipsModified(NEW_CLIP, [OLD_CLIP]);

        await waitFor(() => expect(listedClips()).toEqual([NEW_ROW, MID_ROW]));
        expect(usePlaybackStore.getState().selected).toBe(1);
        expect(selectedRow()).toBe(MID_ROW);
        expect(invokedCommands()).not.toContain("playback_stop");
    });

    it("stops playback when the playing clip is evicted", async () => {
        makeAvailable();
        usePlaybackStore.setState({
            selected: 1,
            status: {
                id: OLD_CLIP.id,
                status: "playing",
                continuously: false,
                sayAgain: false,
                progress: 0.5,
            },
        });
        await renderWithClips([MID_CLIP, OLD_CLIP]);

        emitClipsModified(NEW_CLIP, [OLD_CLIP, MID_CLIP]);

        await waitFor(() => expect(invoke).toHaveBeenCalledWith("playback_stop", undefined));
        expect(listedClips()).toEqual([NEW_ROW]);
    });

    it("leaves a selection that points past the end of the list alone", async () => {
        makeAvailable();
        usePlaybackStore.setState({selected: 3});
        await renderWithClips([MID_CLIP, OLD_CLIP]);

        emitClipsModified(NEW_CLIP);

        await waitFor(() => expect(listedClips()).toEqual([NEW_ROW, MID_ROW, OLD_ROW]));
        expect(usePlaybackStore.getState().selected).toBe(3);
    });
});
