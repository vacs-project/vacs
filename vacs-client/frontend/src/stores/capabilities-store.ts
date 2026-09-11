import {create} from "zustand/react";
import {Capabilities} from "../types/capabilities.ts";
import {invokeStrict} from "../error.ts";

type CapabilitiesState = Capabilities & {
    setCapabilities: (capabilities: Capabilities) => void;
};

export const useCapabilitiesStore = create<CapabilitiesState>()(set => ({
    alwaysOnTop: false,
    keybindListener: false,
    keybindEmitter: false,
    joystick: false,
    playback: false,
    platform: "Unknown",
    setCapabilities: capabilities => set({...capabilities}),
}));

export const fetchCapabilities = async () => {
    try {
        const capabilities = await invokeStrict<Capabilities>("app_platform_capabilities");

        useCapabilitiesStore.getState().setCapabilities(capabilities);
    } catch {}
};

/// Wayland desktops bind keys through the portal, but only when the portal offers global
/// shortcuts; without it the fields fall back to joystick-only capture.
export function selectPortalShortcuts(state: CapabilitiesState): boolean {
    return state.platform === "LinuxWayland" && state.keybindListener;
}
