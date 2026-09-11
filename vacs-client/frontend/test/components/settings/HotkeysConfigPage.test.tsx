import {afterEach, beforeEach, describe, expect, it, vi} from "vitest";
import {cleanup, render, screen, waitFor} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import HotkeysConfigPage from "../../../src/components/settings/HotkeysConfigPage.tsx";
import {useCapabilitiesStore} from "../../../src/stores/capabilities-store.ts";
import {useErrorOverlayStore} from "../../../src/stores/error-overlay-store.ts";
import {Capabilities} from "../../../src/types/capabilities.ts";
import {KeybindsConfig} from "../../../src/types/keybinds.ts";

const KEYBINDS_CONFIG: KeybindsConfig = {acceptCall: null, endCall: null, toggleRadioPrio: null};

const DEFAULT_CAPABILITIES: Capabilities = {
    alwaysOnTop: false,
    keybindListener: false,
    keybindEmitter: false,
    joystick: false,
    playback: false,
    platform: "Unknown",
};

const WAYLAND_WITHOUT_PORTAL: Capabilities = {
    ...DEFAULT_CAPABILITIES,
    joystick: true,
    platform: "LinuxWayland",
};

const HINT = /Keyboard shortcuts are unavailable on this desktop/;

function invokedCommands(): string[] {
    return invoke.mock.calls.map(([cmd]) => cmd);
}

function externalBindingCalls(): unknown[] {
    return invoke.mock.calls
        .filter(([cmd]) => cmd === "keybinds_get_external_binding")
        .map(([, args]) => args);
}

beforeEach(() => {
    invoke.mockImplementation((cmd: string) => {
        switch (cmd) {
            case "keybinds_get_keybinds_config":
                return Promise.resolve(KEYBINDS_CONFIG);
            case "keybinds_get_external_binding":
                return Promise.resolve(null);
            default:
                return Promise.resolve(undefined);
        }
    });
});

afterEach(() => {
    cleanup();
    useCapabilitiesStore.setState(DEFAULT_CAPABILITIES);
    useErrorOverlayStore.getState().close();
    vi.clearAllMocks();
});

describe("HotkeysConfigPage", () => {
    it("binds joystick-only without fetching portal shortcuts when the portal has none", async () => {
        useCapabilitiesStore.setState(WAYLAND_WITHOUT_PORTAL);

        render(<HotkeysConfigPage />);

        await waitFor(() => expect(screen.getAllByText("Not bound")).toHaveLength(3));
        expect(invokedCommands()).toContain("keybinds_get_keybinds_config");
        expect(invokedCommands()).not.toContain("keybinds_get_external_binding");
        expect(screen.getByText(HINT)).toBeTruthy();
        expect(useErrorOverlayStore.getState().visible).toBe(false);
    });

    it("fetches the portal shortcut of every field when the portal offers them", async () => {
        useCapabilitiesStore.setState({...WAYLAND_WITHOUT_PORTAL, keybindListener: true});

        render(<HotkeysConfigPage />);

        await waitFor(() => expect(externalBindingCalls()).toHaveLength(3));
        expect(externalBindingCalls()).toEqual([
            {keybind: "AcceptCall"},
            {keybind: "EndCall"},
            {keybind: "ToggleRadioPrio"},
        ]);
        expect(screen.queryByText(HINT)).toBeNull();
        expect(useErrorOverlayStore.getState().visible).toBe(false);
    });

    it("shows no hint and fetches no portal shortcuts on Windows", async () => {
        useCapabilitiesStore.setState({
            ...DEFAULT_CAPABILITIES,
            keybindListener: true,
            keybindEmitter: true,
            platform: "Windows",
        });

        render(<HotkeysConfigPage />);

        await waitFor(() => expect(screen.getAllByText("Not bound")).toHaveLength(3));
        expect(invokedCommands()).not.toContain("keybinds_get_external_binding");
        expect(screen.queryByText(HINT)).toBeNull();
    });
});
