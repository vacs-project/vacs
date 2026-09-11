import {afterEach, beforeEach, describe, expect, it, vi} from "vitest";
import {cleanup, render, screen, waitFor} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import TransmitModePage from "../../../src/components/settings/TransmitModePage.tsx";
import {useCapabilitiesStore} from "../../../src/stores/capabilities-store.ts";
import {useErrorOverlayStore} from "../../../src/stores/error-overlay-store.ts";
import {useSettingsStore} from "../../../src/stores/settings-store.ts";
import {Capabilities} from "../../../src/types/capabilities.ts";
import {RadioConfigWithLabels, TransmitConfigWithLabels} from "../../../src/types/transmit.ts";

const TRANSMIT_CONFIG: TransmitConfigWithLabels = {
    callMicMode: "PushToTalk",
    pushToTalk: null,
    pushToMute: null,
    radioPushToTalk: null,
    pushToTalkLabel: null,
    pushToMuteLabel: null,
    radioPushToTalkLabel: null,
};

const RADIO_CONFIG: RadioConfigWithLabels = {
    integration: "TrackAudio",
    audioForVatsim: null,
    trackAudio: {endpoint: "localhost:49080"},
};

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

function callMicModeSelect(): HTMLSelectElement | null {
    return document.querySelector<HTMLSelectElement>("select[name='keybind-mode']");
}

beforeEach(() => {
    useSettingsStore.setState({transmitConfig: TRANSMIT_CONFIG, radioConfig: RADIO_CONFIG});
    invoke.mockImplementation((cmd: string) => {
        switch (cmd) {
            case "keybinds_is_portal_shortcut_bound":
                return Promise.resolve(true);
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
    useSettingsStore.setState({transmitConfig: undefined, radioConfig: undefined});
    useErrorOverlayStore.getState().close();
    vi.clearAllMocks();
});

describe("TransmitModePage", () => {
    it("keeps the keybind fields usable when the portal has no global shortcuts", async () => {
        useCapabilitiesStore.setState(WAYLAND_WITHOUT_PORTAL);

        render(<TransmitModePage />);

        expect(screen.getByText(HINT)).toBeTruthy();
        expect(callMicModeSelect()?.value).toBe("PushToTalk");
        expect(screen.queryByText("Not available.")).toBeNull();
        await waitFor(() => expect(screen.getAllByText("Not bound")).toHaveLength(2));
        expect(invokedCommands()).not.toContain("keybinds_get_external_binding");
        expect(invokedCommands()).not.toContain("keybinds_is_portal_shortcut_bound");
        expect(useErrorOverlayStore.getState().visible).toBe(false);
    });

    it("shows the portal shortcuts and no hint when the portal offers global shortcuts", async () => {
        useCapabilitiesStore.setState({...WAYLAND_WITHOUT_PORTAL, keybindListener: true});

        render(<TransmitModePage />);

        await waitFor(() => expect(externalBindingCalls()).toHaveLength(2));
        expect(externalBindingCalls()).toEqual([
            {keybind: "PushToTalk"},
            {keybind: "RadioPushToTalk"},
        ]);
        expect(screen.queryByText(HINT)).toBeNull();
        expect(callMicModeSelect()?.value).toBe("PushToTalk");
    });

    it("offers no keybind fields when neither keyboard nor joystick input is available", () => {
        useCapabilitiesStore.setState({...DEFAULT_CAPABILITIES, platform: "LinuxWayland"});

        render(<TransmitModePage />);

        expect(screen.getByText(HINT)).toBeTruthy();
        expect(screen.getAllByText("Not available.")).toHaveLength(2);
        expect(callMicModeSelect()).toBeNull();
    });
});
