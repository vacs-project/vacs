import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen, transport} = vi.hoisted(() => {
    const transport = {isTauri: true};
    return {
        invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
            Promise.resolve(undefined),
        ),
        listen: vi.fn<
            (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
        >(() => Promise.resolve(() => {})),
        transport,
    };
});

vi.mock("../../src/transport", () => ({
    invoke,
    listen,
    get isTauri() {
        return transport.isTauri;
    },
    isRemote: () => !transport.isTauri,
}));

import {setupPlaybackListener} from "../../src/listeners/playback-listener.ts";
import {PlaybackStatus, usePlaybackStore} from "../../src/stores/playback-store.ts";
import {ClipMeta} from "../../src/types/playback.ts";

const CLIP: ClipMeta = {
    id: 7,
    path: "/tmp/clip.wav",
    callsigns: ["VIE_TWR"],
    frequency: 118_300_000,
    startedAt: {secs_since_epoch: 0, nanos_since_epoch: 0},
    endedAt: {secs_since_epoch: 1, nanos_since_epoch: 0},
    durationMs: 1000,
};

const SAY_AGAIN_STATUS: PlaybackStatus = {
    id: 7,
    status: "playing",
    continuously: false,
    sayAgain: true,
    progress: 0.3,
};

function findSayAgainCallback() {
    const call = listen.mock.calls.find(([event]) => event === "playback:say-again");
    if (call === undefined) throw new Error("playback:say-again was never registered");
    return call[1];
}

afterEach(() => {
    transport.isTauri = true;
    usePlaybackStore.setState({
        selected: 0,
        status: undefined,
        playbackDevice: "Output",
        openInstanceIds: [],
    });
    vi.clearAllMocks();
});

describe("setupPlaybackListener", () => {
    it("starts a say-again replay on the desktop when the key binding fires", async () => {
        usePlaybackStore.setState({playbackDevice: "Speaker"});
        invoke.mockImplementation((cmd: string) =>
            cmd === "playback_say_again" ? Promise.resolve(CLIP) : Promise.resolve(undefined),
        );
        const teardown = setupPlaybackListener();

        findSayAgainCallback()({payload: null});

        await vi.waitFor(() =>
            expect(invoke).toHaveBeenCalledWith("playback_say_again", {deviceType: "Speaker"}),
        );
        await vi.waitFor(() =>
            expect(usePlaybackStore.getState().status).toEqual({
                id: 7,
                status: "playing",
                continuously: false,
                sayAgain: true,
                progress: 0,
            }),
        );

        teardown();
    });

    it("stops the running say-again replay on a second press", async () => {
        usePlaybackStore.setState({status: SAY_AGAIN_STATUS});
        const teardown = setupPlaybackListener();

        findSayAgainCallback()({payload: null});

        await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("playback_stop", undefined));
        await vi.waitFor(() => expect(usePlaybackStore.getState().status).toBeUndefined());

        teardown();
    });

    it("leaves the status untouched when the backend has nothing to replay", async () => {
        const otherClip: PlaybackStatus = {...SAY_AGAIN_STATUS, id: 2, sayAgain: false};
        usePlaybackStore.setState({status: otherClip});
        invoke.mockImplementation((cmd: string) =>
            cmd === "playback_say_again" ? Promise.resolve(null) : Promise.resolve(undefined),
        );
        const teardown = setupPlaybackListener();

        findSayAgainCallback()({payload: null});

        await vi.waitFor(() =>
            expect(invoke).toHaveBeenCalledWith("playback_say_again", expect.anything()),
        );
        await new Promise(resolve => setTimeout(resolve, 0));
        expect(usePlaybackStore.getState().status).toEqual(otherClip);

        teardown();
    });

    it("does not listen in a remote session", () => {
        transport.isTauri = false;
        const teardown = setupPlaybackListener();

        expect(listen).not.toHaveBeenCalledWith("playback:say-again", expect.any(Function));

        teardown();
    });
});
