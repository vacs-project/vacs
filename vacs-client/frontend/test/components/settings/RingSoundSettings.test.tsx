import {afterEach, beforeEach, describe, expect, it, vi} from "vitest";
import {cleanup, fireEvent, render, screen, waitFor, within} from "@testing-library/preact";

const {invoke, listen, state} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
    state: {isTauri: true},
}));

vi.mock("../../../src/transport", () => ({
    invoke,
    listen,
    get isTauri() {
        return state.isTauri;
    },
    isRemote: () => !state.isTauri,
}));

import RingSoundSettings from "../../../src/components/settings/RingSoundSettings.tsx";
import {useErrorOverlayStore} from "../../../src/stores/error-overlay-store.ts";
import {useSettingsStore} from "../../../src/stores/settings-store.ts";

const initialCallConfig = useSettingsStore.getState().callConfig;

beforeEach(() => {
    state.isTauri = true;
    useSettingsStore.setState({callConfig: {...initialCallConfig, enablePriorityCalls: true}});
});

afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    useErrorOverlayStore.getState().close();
});

function mockInvoke(handlers: Record<string, (args?: Record<string, unknown>) => unknown>) {
    invoke.mockImplementation((cmd, args) => Promise.resolve(handlers[cmd]?.(args)));
}

const calledCommands = () => invoke.mock.calls.map(call => call[0]);
const pickCalls = () => calledCommands().filter(cmd => cmd === "audio_pick_ring_sound");
// The grid renders the ring field first and the priority ring field second.
const grid = () => screen.getByText("Ring").parentElement!;
const field = (index: 0 | 1) =>
    within(grid()).getAllByText(/Built-in chime|\.wav$/)[index].parentElement!;
const remove = (index: 0 | 1) => field(index).nextElementSibling!;

describe("RingSoundSettings", () => {
    it("shows a loading state until the backend answers", () => {
        invoke.mockImplementation(() => new Promise(() => {}));
        render(<RingSoundSettings />);

        expect(screen.getAllByText("Loading...")).toHaveLength(2);
        expect(screen.queryByText("Built-in chime")).toBeNull();
    });

    it("shows the built-in chime and the configured file name", async () => {
        mockInvoke({
            audio_get_ring_sounds: () => ({
                priorityRing: {path: "C:\\Users\\me\\Sounds\\urgent.wav", available: true},
            }),
        });
        render(<RingSoundSettings />);

        await waitFor(() => expect(screen.getByText("urgent.wav")).toBeTruthy());
        expect(screen.getByText("Built-in chime")).toBeTruthy();
        expect(field(0).className).toContain("text-gray-500");
        expect(field(0).className).toContain("cursor-pointer");
        expect(field(1).getAttribute("title")).toBe("C:\\Users\\me\\Sounds\\urgent.wav");
        expect(remove(0).getAttribute("class")).toContain("cursor-not-allowed");
        expect(remove(1).getAttribute("class")).toContain("cursor-pointer");
    });

    it("flags a configured file that could not be loaded", async () => {
        mockInvoke({
            audio_get_ring_sounds: () => ({ring: {path: "/gone/ring.wav", available: false}}),
        });
        render(<RingSoundSettings />);

        await waitFor(() => expect(screen.getByText("ring.wav")).toBeTruthy());
        expect(field(0).className).toContain("text-red-700");
        expect(field(0).getAttribute("title")).toContain("could not be loaded");
        expect(remove(0).getAttribute("class")).toContain("cursor-pointer");
    });

    it("applies the picked file and shows the result", async () => {
        mockInvoke({
            audio_get_ring_sounds: () => ({}),
            audio_pick_ring_sound: () => "/home/me/ring.wav",
            audio_set_ring_sound: args => ({ring: {path: args?.path, available: true}}),
        });
        render(<RingSoundSettings />);
        await waitFor(() => expect(screen.getAllByText("Built-in chime")).toHaveLength(2));

        fireEvent.click(field(0));

        await waitFor(() =>
            expect(invoke).toHaveBeenCalledWith("audio_set_ring_sound", {
                ringType: "ring",
                path: "/home/me/ring.wav",
            }),
        );
        expect(invoke).toHaveBeenCalledWith("audio_pick_ring_sound", undefined);
        await waitFor(() => expect(screen.getByText("ring.wav")).toBeTruthy());
    });

    it("does not change anything when the file dialog is cancelled", async () => {
        mockInvoke({
            audio_get_ring_sounds: () => ({}),
            audio_pick_ring_sound: () => null,
        });
        render(<RingSoundSettings />);
        await waitFor(() => expect(screen.getAllByText("Built-in chime")).toHaveLength(2));

        fireEvent.click(field(1));

        await waitFor(() => expect(pickCalls()).toHaveLength(1));
        expect(calledCommands()).not.toContain("audio_set_ring_sound");
    });

    it("opens only one file dialog while a pick is in flight", async () => {
        let resolvePick: (path: string | null) => void = () => {};
        mockInvoke({audio_get_ring_sounds: () => ({})});
        render(<RingSoundSettings />);
        await waitFor(() => expect(screen.getAllByText("Built-in chime")).toHaveLength(2));
        invoke.mockImplementation(cmd =>
            cmd === "audio_pick_ring_sound"
                ? new Promise(resolve => {
                      resolvePick = resolve;
                  })
                : Promise.resolve(undefined),
        );

        fireEvent.click(field(0));
        fireEvent.click(field(0));
        await waitFor(() => expect(pickCalls()).toHaveLength(1));
        resolvePick(null);

        expect(pickCalls()).toHaveLength(1);
    });

    it("keeps the previous sound when the backend rejects the file", async () => {
        mockInvoke({
            audio_get_ring_sounds: () => ({ring: {path: "/home/me/old.wav", available: true}}),
        });
        render(<RingSoundSettings />);
        await waitFor(() => expect(screen.getByText("old.wav")).toBeTruthy());
        invoke.mockImplementation(cmd =>
            cmd === "audio_set_ring_sound"
                ? Promise.reject({title: "Error", detail: "too long", isNonCritical: false})
                : cmd === "audio_pick_ring_sound"
                  ? Promise.resolve("/home/me/long.wav")
                  : Promise.resolve(undefined),
        );

        fireEvent.click(field(0));

        await waitFor(() => expect(calledCommands()).toContain("audio_set_ring_sound"));
        expect(screen.getByText("old.wav")).toBeTruthy();
        expect(screen.queryByText("long.wav")).toBeNull();
        await waitFor(() => expect(useErrorOverlayStore.getState().visible).toBe(true));
    });

    it("resets to the built-in chime", async () => {
        mockInvoke({
            audio_get_ring_sounds: () => ({ring: {path: "/home/me/ring.wav", available: true}}),
            audio_set_ring_sound: () => ({}),
        });
        render(<RingSoundSettings />);
        await waitFor(() => expect(screen.getByText("ring.wav")).toBeTruthy());

        fireEvent.click(remove(0));

        await waitFor(() =>
            expect(invoke).toHaveBeenCalledWith("audio_set_ring_sound", {
                ringType: "ring",
                path: null,
            }),
        );
        await waitFor(() => expect(screen.queryByText("ring.wav")).toBeNull());
        expect(screen.getAllByText("Built-in chime")).toHaveLength(2);
    });

    it("ignores the reset when the built-in chime is already set", async () => {
        mockInvoke({audio_get_ring_sounds: () => ({})});
        render(<RingSoundSettings />);
        await waitFor(() => expect(screen.getAllByText("Built-in chime")).toHaveLength(2));

        fireEvent.click(remove(0));

        expect(calledCommands()).not.toContain("audio_set_ring_sound");
    });

    it("disables the priority ring while priority calls are off", async () => {
        useSettingsStore.setState({callConfig: {...initialCallConfig, enablePriorityCalls: false}});
        mockInvoke({
            audio_get_ring_sounds: () => ({
                priorityRing: {path: "/home/me/prio.wav", available: true},
            }),
        });
        render(<RingSoundSettings />);
        await waitFor(() => expect(screen.getByText("prio.wav")).toBeTruthy());

        expect(field(0).className).toContain("cursor-pointer");
        expect(field(1).className).toContain("cursor-not-allowed");
        expect(field(1).getAttribute("title")).toContain("Enable priority calls");
        expect(remove(1).getAttribute("class")).toContain("cursor-not-allowed");
        fireEvent.click(field(1));
        fireEvent.click(remove(1));
        expect(pickCalls()).toHaveLength(0);
        expect(calledCommands()).not.toContain("audio_set_ring_sound");
    });

    it("disables choosing a file in a remote session but still allows the reset", async () => {
        state.isTauri = false;
        mockInvoke({
            audio_get_ring_sounds: () => ({ring: {path: "/home/me/ring.wav", available: true}}),
            audio_set_ring_sound: () => ({}),
        });
        render(<RingSoundSettings />);
        await waitFor(() => expect(screen.getByText("ring.wav")).toBeTruthy());

        expect(field(0).className).toContain("cursor-not-allowed");
        expect(field(0).getAttribute("title")).toContain("/home/me/ring.wav");
        expect(field(0).getAttribute("title")).toContain("desktop client");
        fireEvent.click(field(0));
        expect(pickCalls()).toHaveLength(0);

        fireEvent.click(remove(0));
        await waitFor(() =>
            expect(invoke).toHaveBeenCalledWith("audio_set_ring_sound", {
                ringType: "ring",
                path: null,
            }),
        );
    });
});
