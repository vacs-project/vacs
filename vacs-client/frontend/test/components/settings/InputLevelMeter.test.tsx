import {afterEach, describe, expect, it, vi} from "vitest";
import {act, cleanup, render, waitFor} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import InputLevelMeter from "../../../src/components/settings/InputLevelMeter.tsx";

function invokedCommands(): string[] {
    return invoke.mock.calls.map(([cmd]) => cmd);
}

afterEach(() => {
    cleanup();
    vi.clearAllMocks();
});

describe("InputLevelMeter", () => {
    it("stops the level meter when unmounted", async () => {
        const {unmount} = render(<InputLevelMeter />);
        await waitFor(() => expect(invokedCommands()).toContain("audio_start_input_level_meter"));
        expect(invokedCommands()).not.toContain("audio_stop_input_level_meter");

        await act(() => {
            unmount();
        });

        await waitFor(() => expect(invokedCommands()).toContain("audio_stop_input_level_meter"));
    });
});
