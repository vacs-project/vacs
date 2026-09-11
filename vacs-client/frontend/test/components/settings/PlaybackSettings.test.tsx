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
    useSettingsStore.setState({playbackEnabled: false});
    usePlaybackStore.setState({selected: 0, status: undefined});
    vi.clearAllMocks();
    cleanup();
});

describe("PlaybackSettings", () => {
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
