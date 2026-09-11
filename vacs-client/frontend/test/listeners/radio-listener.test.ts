import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import {setupRadioListener} from "../../src/listeners/radio-listener.ts";
import {usePlaybackStore, PlaybackStatus} from "../../src/stores/playback-store.ts";
import {useRadioStore} from "../../src/stores/radio-store.ts";

const SAY_AGAIN_STATUS: PlaybackStatus = {
    id: 7,
    status: "playing",
    continuously: false,
    sayAgain: true,
    progress: 0.3,
};

function findRadioStateCallback() {
    const call = listen.mock.calls.find(([event]) => event === "radio:state");
    if (call === undefined) throw new Error("radio:state was never registered");
    return call[1];
}

afterEach(() => {
    useRadioStore.setState({radioState: undefined});
    usePlaybackStore.setState({
        selected: 0,
        status: undefined,
        playbackDevice: "Output",
        openInstanceIds: [],
    });
    vi.clearAllMocks();
});

describe("setupRadioListener", () => {
    it("clears the playback status when the radio disconnects", () => {
        usePlaybackStore.setState({status: SAY_AGAIN_STATUS});
        const teardown = setupRadioListener();

        findRadioStateCallback()({payload: {state: "Disconnected"}});

        expect(usePlaybackStore.getState().status).toBeUndefined();
        expect(useRadioStore.getState().radioState).toEqual({state: "Disconnected"});

        teardown();
    });

    it("clears the playback status when the radio becomes unconfigured", () => {
        usePlaybackStore.setState({status: SAY_AGAIN_STATUS});
        const teardown = setupRadioListener();

        findRadioStateCallback()({payload: {state: "NotConfigured"}});

        expect(usePlaybackStore.getState().status).toBeUndefined();

        teardown();
    });

    it("leaves the playback status alone when the radio is connected", () => {
        usePlaybackStore.setState({status: SAY_AGAIN_STATUS});
        const teardown = setupRadioListener();

        findRadioStateCallback()({payload: {state: "Connected"}});

        expect(usePlaybackStore.getState().status).toEqual(SAY_AGAIN_STATUS);
        expect(useRadioStore.getState().radioState).toEqual({state: "Connected"});

        teardown();
    });
});
