import {afterEach, describe, expect, it, vi} from "vitest";
import {act, renderHook} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import {usePlaybackControls} from "../../src/hooks/playback-controls-hook.ts";
import {usePlaybackStore} from "../../src/stores/playback-store.ts";
import {INSTANCE_ID} from "../../src/transport/store-sync.ts";
import {ClipMeta} from "../../src/types/playback.ts";

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

function useTestControls(clips: ClipMeta[]) {
    const selected = usePlaybackStore(state => state.selected);
    return usePlaybackControls({
        clips,
        selectedClip: clips[selected],
        prevClip: clips[selected + 1],
        nextClip: clips[selected - 1],
    });
}

afterEach(() => {
    usePlaybackStore.setState({
        selected: 0,
        status: undefined,
        playbackDevice: "Output",
        openInstanceIds: [],
    });
    vi.clearAllMocks();
});

describe("usePlaybackControls", () => {
    it("reconciles selection to a clip started elsewhere without stopping it", () => {
        const clips = [makeClip({id: 9}), makeClip({id: 2})];
        usePlaybackStore.setState({
            selected: 1,
            status: {id: 2, status: "playing", continuously: false, sayAgain: false, progress: 0},
            openInstanceIds: [INSTANCE_ID],
        });
        const {rerender} = renderHook((c: ClipMeta[]) => useTestControls(c), {initialProps: clips});

        void act(() => {
            usePlaybackStore.setState({
                status: {
                    id: 9,
                    status: "playing",
                    continuously: false,
                    sayAgain: true,
                    progress: 0,
                },
            });
        });
        rerender(clips);

        expect(invoke).not.toHaveBeenCalledWith("playback_stop", expect.anything());
        expect(usePlaybackStore.getState().selected).toBe(0);
    });

    it("reconciles a stale selection already present on mount", () => {
        const clips = [makeClip({id: 9}), makeClip({id: 2})];
        usePlaybackStore.setState({
            selected: 0,
            status: {id: 2, status: "playing", continuously: false, sayAgain: true, progress: 0},
            openInstanceIds: [INSTANCE_ID],
        });

        renderHook((c: ClipMeta[]) => useTestControls(c), {initialProps: clips});

        expect(usePlaybackStore.getState().selected).toBe(1);
        expect(invoke).not.toHaveBeenCalledWith("playback_stop", expect.anything());
    });

    it("still stops playback when the user selects a different row", () => {
        const clips = [makeClip({id: 9}), makeClip({id: 2})];
        usePlaybackStore.setState({
            selected: 0,
            status: {id: 9, status: "playing", continuously: false, sayAgain: false, progress: 0},
            openInstanceIds: [INSTANCE_ID],
        });
        const {rerender} = renderHook((c: ClipMeta[]) => useTestControls(c), {initialProps: clips});

        void act(() => {
            usePlaybackStore.setState({selected: 1});
        });
        rerender(clips);

        expect(invoke).toHaveBeenCalledWith("playback_stop", undefined);
    });

    it("leaves selection and playback alone when the status clip is not in the list", () => {
        const clips = [makeClip({id: 9}), makeClip({id: 2})];
        usePlaybackStore.setState({selected: 0, openInstanceIds: [INSTANCE_ID]});
        const {rerender} = renderHook((c: ClipMeta[]) => useTestControls(c), {initialProps: clips});

        void act(() => {
            usePlaybackStore.setState({
                status: {
                    id: 5,
                    status: "playing",
                    continuously: false,
                    sayAgain: false,
                    progress: 0,
                },
            });
        });
        rerender(clips);

        expect(usePlaybackStore.getState().selected).toBe(0);
        expect(invoke).not.toHaveBeenCalledWith("playback_stop", expect.anything());
    });
});
