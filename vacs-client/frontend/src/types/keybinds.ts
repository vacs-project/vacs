import {CallMicMode, InputBinding} from "./transmit.ts";

export type KeybindType =
    | "PushToTalk"
    | "PushToMute"
    | "RadioPushToTalk"
    | "AcceptCall"
    | "EndCall"
    | "ToggleRadioPrio"
    | "SayAgain";

/// A joystick device identified by its stable SDL GUID. `name` is the
/// last-seen device name, kept for display while the device is unplugged.
export type JoystickDevice = {
    device: string;
    name?: string;
};

/// A joystick device with its presence and capture-ignore state, as returned
/// by keybinds_list_joystick_devices: all connected devices plus ignored ones
/// that are currently unplugged.
export type JoystickDeviceEntry = JoystickDevice & {
    connected: boolean;
    ignored: boolean;
};

export type KeybindsConfig = {
    acceptCall: InputBinding | null;
    endCall: InputBinding | null;
    toggleRadioPrio: InputBinding | null;
    sayAgain: InputBinding | null;
};

export function callMicModeToKeybind(mode: CallMicMode): KeybindType | null {
    switch (mode) {
        case "PushToTalk":
            return "PushToTalk";
        case "PushToMute":
            return "PushToMute";
        case "VoiceActivation":
            return null;
    }
}
