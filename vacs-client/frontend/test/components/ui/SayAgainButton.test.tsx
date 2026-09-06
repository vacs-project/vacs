import {afterEach, beforeEach, describe, expect, it, vi} from "vitest";
import {act, cleanup, fireEvent, render, screen, waitFor} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import {ButtonColors} from "../../../src/components/ui/Button.tsx";
import SayAgainButton from "../../../src/components/ui/SayAgainButton.tsx";
import {useCapabilitiesStore} from "../../../src/stores/capabilities-store.ts";
import {usePlaybackStore} from "../../../src/stores/playback-store.ts";
import {useRadioStore} from "../../../src/stores/radio-store.ts";
import {useSettingsStore} from "../../../src/stores/settings-store.ts";
import {ClipMeta} from "../../../src/types/playback.ts";
import {RadioConfigWithLabels} from "../../../src/types/transmit.ts";

const RADIO_CONFIG: RadioConfigWithLabels = {
    integration: "TrackAudio",
    audioForVatsim: null,
    trackAudio: {endpoint: "ws://localhost:49080"},
};

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

function makeAvailable() {
    useCapabilitiesStore.setState({playback: true});
    useSettingsStore.setState({radioConfig: RADIO_CONFIG, playbackEnabled: true});
    useRadioStore.setState({radioState: {state: "Connected"}});
}

function getButton() {
    return screen.getByRole("button", {name: "SAYAGAIN"});
}

function findProgressCallback() {
    const call = listen.mock.calls.find(([event]) => event === "playback:progress");
    if (call === undefined) throw new Error("playback:progress was never registered");
    return call[1];
}

beforeEach(() => {
    invoke.mockImplementation(() => Promise.resolve(undefined));
});

afterEach(() => {
    useCapabilitiesStore.setState({playback: false});
    useSettingsStore.setState({
        radioConfig: undefined,
        playbackEnabled: false,
        sayAgainEnabled: false,
    });
    useRadioStore.setState({radioState: undefined});
    usePlaybackStore.setState({
        selected: 0,
        status: undefined,
        playbackDevice: "Output",
        openInstanceIds: [],
    });
    vi.clearAllMocks();
    cleanup();
});

describe("SayAgainButton", () => {
    describe("availability", () => {
        it("is disabled and muted when the playback capability is missing", () => {
            useSettingsStore.setState({radioConfig: RADIO_CONFIG, playbackEnabled: true});
            useRadioStore.setState({radioState: {state: "Connected"}});
            render(<SayAgainButton />);

            const btn = getButton() as HTMLButtonElement;
            expect(btn.disabled).toBe(true);
            expect(btn).toHaveClasses("text-slate-400");
        });

        it("is disabled and muted when no radio integration is configured", () => {
            useCapabilitiesStore.setState({playback: true});
            useSettingsStore.setState({radioConfig: undefined, playbackEnabled: true});
            useRadioStore.setState({radioState: {state: "Connected"}});
            render(<SayAgainButton />);

            const btn = getButton() as HTMLButtonElement;
            expect(btn.disabled).toBe(true);
            expect(btn).toHaveClasses("text-slate-400");
        });

        it("is disabled and muted when playback is not enabled in settings", () => {
            useCapabilitiesStore.setState({playback: true});
            useSettingsStore.setState({radioConfig: RADIO_CONFIG, playbackEnabled: false});
            useRadioStore.setState({radioState: {state: "Connected"}});
            render(<SayAgainButton />);

            const btn = getButton() as HTMLButtonElement;
            expect(btn.disabled).toBe(true);
            expect(btn).toHaveClasses("text-slate-400");
        });

        it("is disabled but not muted when the radio is disconnected", () => {
            makeAvailable();
            useRadioStore.setState({radioState: {state: "Disconnected"}});
            render(<SayAgainButton />);

            const btn = getButton() as HTMLButtonElement;
            expect(btn.disabled).toBe(true);
            expect(btn.classList.contains("text-slate-400")).toBe(false);
        });

        it("is disabled but not muted when the radio is not configured", () => {
            makeAvailable();
            useRadioStore.setState({radioState: {state: "NotConfigured"}});
            render(<SayAgainButton />);

            const btn = getButton() as HTMLButtonElement;
            expect(btn.disabled).toBe(true);
            expect(btn.classList.contains("text-slate-400")).toBe(false);
        });

        it("is enabled when playback is available and the radio is connected", () => {
            makeAvailable();
            render(<SayAgainButton />);

            const btn = getButton() as HTMLButtonElement;
            expect(btn.disabled).toBe(false);
            expect(btn.classList.contains("text-slate-400")).toBe(false);
        });
    });

    describe("press when idle", () => {
        it("starts say-again playback, applies the clip and turns blue", async () => {
            makeAvailable();
            usePlaybackStore.setState({playbackDevice: "Output", selected: 3});
            const clip = makeClip({id: 7});
            invoke.mockImplementation((cmd: string) =>
                cmd === "playback_say_again" ? Promise.resolve(clip) : Promise.resolve(undefined),
            );
            render(<SayAgainButton />);

            fireEvent.click(getButton());

            await waitFor(() =>
                expect(invoke).toHaveBeenCalledWith("playback_say_again", {deviceType: "Output"}),
            );
            await waitFor(() =>
                expect(usePlaybackStore.getState().status).toEqual({
                    id: 7,
                    status: "playing",
                    continuously: false,
                    sayAgain: true,
                    progress: 0,
                }),
            );
            expect(usePlaybackStore.getState().selected).toBe(3);
            await waitFor(() => expect(getButton()).toHaveClasses(ButtonColors.blue));
        });

        it("does nothing when the backend returns no clip", async () => {
            makeAvailable();
            invoke.mockImplementation((cmd: string) =>
                cmd === "playback_say_again" ? Promise.resolve(null) : Promise.resolve(undefined),
            );
            render(<SayAgainButton />);

            fireEvent.click(getButton());

            await waitFor(() =>
                expect(invoke).toHaveBeenCalledWith("playback_say_again", expect.anything()),
            );
            expect(usePlaybackStore.getState().status).toBeUndefined();
        });
    });

    describe("press while playing", () => {
        it("stops playback and clears status when the say-again clip is playing", async () => {
            makeAvailable();
            usePlaybackStore.setState({
                status: {
                    id: 3,
                    status: "playing",
                    continuously: false,
                    sayAgain: true,
                    progress: 0.4,
                },
            });
            render(<SayAgainButton />);
            expect(getButton()).toHaveClasses(ButtonColors.blue);

            fireEvent.click(getButton());

            await waitFor(() => expect(invoke).toHaveBeenCalledWith("playback_stop", undefined));
            await waitFor(() => expect(usePlaybackStore.getState().status).toBeUndefined());
            expect(getButton()).toHaveClasses(ButtonColors.cyan);
        });

        it("stops playback and clears status when the say-again clip is paused", async () => {
            makeAvailable();
            usePlaybackStore.setState({
                status: {
                    id: 3,
                    status: "paused",
                    continuously: false,
                    sayAgain: true,
                    progress: 0.4,
                },
            });
            render(<SayAgainButton />);
            expect(getButton()).toHaveClasses(ButtonColors.blue);

            fireEvent.click(getButton());

            await waitFor(() => expect(invoke).toHaveBeenCalledWith("playback_stop", undefined));
            await waitFor(() => expect(usePlaybackStore.getState().status).toBeUndefined());
            expect(getButton()).toHaveClasses(ButtonColors.cyan);
        });

        it("starts a new say-again clip instead of stopping when a different clip is playing", async () => {
            makeAvailable();
            usePlaybackStore.setState({
                status: {
                    id: 2,
                    status: "playing",
                    continuously: false,
                    sayAgain: false,
                    progress: 0.1,
                },
            });
            const clip = makeClip({id: 9});
            invoke.mockImplementation((cmd: string) =>
                cmd === "playback_say_again" ? Promise.resolve(clip) : Promise.resolve(undefined),
            );
            render(<SayAgainButton />);

            fireEvent.click(getButton());

            await waitFor(() =>
                expect(invoke).toHaveBeenCalledWith("playback_say_again", {deviceType: "Output"}),
            );
            expect(invoke).not.toHaveBeenCalledWith("playback_stop", expect.anything());
        });
    });

    describe("progress events", () => {
        it("clears status once progress reaches completion and no instance owns the playback page", async () => {
            makeAvailable();
            usePlaybackStore.setState({
                status: {
                    id: 5,
                    status: "playing",
                    continuously: false,
                    sayAgain: true,
                    progress: 0.2,
                },
                openInstanceIds: [],
            });
            render(<SayAgainButton />);
            await waitFor(() =>
                expect(listen).toHaveBeenCalledWith("playback:progress", expect.any(Function)),
            );
            const callback = findProgressCallback();

            void act(() => callback({payload: 0.5}));
            expect(usePlaybackStore.getState().status).toBeDefined();

            void act(() => callback({payload: 1}));
            expect(usePlaybackStore.getState().status).toBeUndefined();
        });

        it("ignores progress completion while another instance owns the playback page", async () => {
            makeAvailable();
            usePlaybackStore.setState({
                status: {
                    id: 5,
                    status: "playing",
                    continuously: false,
                    sayAgain: true,
                    progress: 0.2,
                },
                openInstanceIds: ["other-instance"],
            });
            render(<SayAgainButton />);
            await waitFor(() =>
                expect(listen).toHaveBeenCalledWith("playback:progress", expect.any(Function)),
            );
            const callback = findProgressCallback();

            void act(() => callback({payload: 1}));
            expect(usePlaybackStore.getState().status).toBeDefined();
        });
    });
});
