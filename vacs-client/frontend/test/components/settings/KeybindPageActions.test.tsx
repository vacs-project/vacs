import {afterEach, describe, expect, it, vi} from "vitest";
import {cleanup, render} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import KeybindPageActions from "../../../src/components/settings/KeybindPageActions.tsx";
import {useCapabilitiesStore} from "../../../src/stores/capabilities-store.ts";
import {Capabilities} from "../../../src/types/capabilities.ts";

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

function buttonLabels(): string[] {
    return Array.from(document.querySelectorAll("button")).map(button => button.textContent ?? "");
}

afterEach(() => {
    cleanup();
    useCapabilitiesStore.setState(DEFAULT_CAPABILITIES);
    vi.clearAllMocks();
});

describe("KeybindPageActions", () => {
    it("offers only joystick devices when the portal has no global shortcuts", () => {
        useCapabilitiesStore.setState(WAYLAND_WITHOUT_PORTAL);

        render(<KeybindPageActions />);

        expect(buttonLabels()).toEqual(["JoystickDevices"]);
    });

    it("offers the system shortcuts settings when the portal offers global shortcuts", () => {
        useCapabilitiesStore.setState({...WAYLAND_WITHOUT_PORTAL, keybindListener: true});

        render(<KeybindPageActions />);

        expect(buttonLabels()).toEqual(["JoystickDevices", "SystemShortcuts"]);
    });

    it("offers only joystick devices on Windows", () => {
        useCapabilitiesStore.setState({
            ...DEFAULT_CAPABILITIES,
            keybindListener: true,
            keybindEmitter: true,
            joystick: true,
            platform: "Windows",
        });

        render(<KeybindPageActions />);

        expect(buttonLabels()).toEqual(["JoystickDevices"]);
    });
});
