import {afterEach, describe, expect, it, vi} from "vitest";
import {cleanup, fireEvent, render, screen, waitFor} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import PlaybackSettings from "../../../src/components/settings/PlaybackSettings.tsx";
import {usePlaybackStore} from "../../../src/stores/playback-store.ts";
import {useSettingsStore} from "../../../src/stores/settings-store.ts";

afterEach(() => {
    useSettingsStore.setState({playbackEnabled: false, sayAgainEnabled: false});
    usePlaybackStore.setState({selected: 0, status: undefined});
    vi.clearAllMocks();
    cleanup();
});

describe("PlaybackSettings", () => {
    it("enables the SAY AGAIN button setting and invokes the backend", async () => {
        useSettingsStore.setState({sayAgainEnabled: false});
        invoke.mockImplementation(() => Promise.resolve(undefined));
        render(<PlaybackSettings />);

        fireEvent.click(screen.getByRole("checkbox", {name: "Show SAY AGAIN button"}));

        await waitFor(() =>
            expect(invoke).toHaveBeenCalledWith("playback_set_say_again", {enabled: true}),
        );
        const calls = invoke.mock.calls.filter(([cmd]) => cmd === "playback_set_say_again");
        expect(calls).toHaveLength(1);
        expect(useSettingsStore.getState().sayAgainEnabled).toBe(true);
    });

    it("rolls back the SAY AGAIN setting when the backend rejects", async () => {
        useSettingsStore.setState({sayAgainEnabled: false});
        invoke.mockImplementation(() => Promise.reject(new Error("nope")));
        render(<PlaybackSettings />);

        fireEvent.click(screen.getByRole("checkbox", {name: "Show SAY AGAIN button"}));

        await waitFor(() => expect(useSettingsStore.getState().sayAgainEnabled).toBe(false));
    });

    it("stops a running say-again replay when the button is hidden", async () => {
        useSettingsStore.setState({playbackEnabled: true, sayAgainEnabled: true});
        usePlaybackStore.setState({
            status: {id: 1, status: "playing", continuously: false, sayAgain: true, progress: 0},
        });
        invoke.mockImplementation(() => Promise.resolve(undefined));
        render(<PlaybackSettings />);

        fireEvent.click(screen.getByRole("checkbox", {name: "Show SAY AGAIN button"}));

        await waitFor(() => expect(invoke).toHaveBeenCalledWith("playback_stop", undefined));
        await waitFor(() => expect(usePlaybackStore.getState().status).toBeUndefined());
        expect(invoke.mock.calls.findIndex(([cmd]) => cmd === "playback_stop")).toBeGreaterThan(
            invoke.mock.calls.findIndex(([cmd]) => cmd === "playback_set_say_again"),
        );
    });

    it("leaves a page-started clip playing when the button is hidden", async () => {
        useSettingsStore.setState({playbackEnabled: true, sayAgainEnabled: true});
        usePlaybackStore.setState({
            status: {id: 1, status: "playing", continuously: false, sayAgain: false, progress: 0},
        });
        invoke.mockImplementation(() => Promise.resolve(undefined));
        render(<PlaybackSettings />);

        fireEvent.click(screen.getByRole("checkbox", {name: "Show SAY AGAIN button"}));

        await waitFor(() =>
            expect(invoke).toHaveBeenCalledWith("playback_set_say_again", {enabled: false}),
        );
        expect(invoke).not.toHaveBeenCalledWith("playback_stop", undefined);
        expect(usePlaybackStore.getState().status?.id).toBe(1);
    });

    it("clears the playback status when radio playback is disabled", async () => {
        useSettingsStore.setState({playbackEnabled: true});
        usePlaybackStore.setState({
            status: {id: 1, status: "playing", continuously: false, sayAgain: false, progress: 0},
        });
        invoke.mockImplementation(() => Promise.resolve(undefined));
        render(<PlaybackSettings />);

        fireEvent.click(screen.getByRole("checkbox", {name: "Enable radio playback"}));

        await waitFor(() =>
            expect(invoke).toHaveBeenCalledWith("playback_set_enabled", {enabled: false}),
        );
        await waitFor(() => expect(usePlaybackStore.getState().status).toBeUndefined());
        expect(useSettingsStore.getState().playbackEnabled).toBe(false);
    });
});
