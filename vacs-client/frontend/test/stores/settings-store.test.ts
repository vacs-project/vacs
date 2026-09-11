import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import {fetchSettings, useSettingsStore} from "../../src/stores/settings-store.ts";
import {CallConfig} from "../../src/types/settings.ts";
import {RadioConfig, TransmitConfig} from "../../src/types/transmit.ts";

const CALL_CONFIG: CallConfig = {
    highlightIncomingCallTarget: false,
    enablePriorityCalls: false,
    enableCallStartSound: false,
    enableCallEndSound: false,
    useDefaultCallSources: true,
    forceRelay: true,
};

const RADIO_CONFIG: RadioConfig = {
    integration: "TrackAudio",
    audioForVatsim: null,
    trackAudio: {endpoint: "ws://localhost:49080"},
};

const TRANSMIT_CONFIG: TransmitConfig = {
    callMicMode: "PushToTalk",
    pushToTalk: "KeyA",
    pushToMute: null,
    radioPushToTalk: null,
};

const RESPONSES: Record<string, unknown> = {
    app_get_client_page_settings: {selected: undefined, configs: {}},
    app_get_call_config: CALL_CONFIG,
    app_get_clock_mode: "Simulated",
    app_get_cpl_mode: "Fast",
    keybinds_get_transmit_config: TRANSMIT_CONFIG,
    radio_get_config: RADIO_CONFIG,
    playback_get_enabled: true,
};

afterEach(() => {
    useSettingsStore.setState({
        clockMode: "Realtime",
        cplMode: "Original",
        transmitConfig: undefined,
        radioConfig: undefined,
        playbackEnabled: false,
    });
    vi.clearAllMocks();
});

describe("settings store", () => {
    it("fetchSettings maps every command response to its store field", async () => {
        invoke.mockImplementation((cmd: string) => Promise.resolve(RESPONSES[cmd]));

        await fetchSettings();

        const state = useSettingsStore.getState();
        expect(state.callConfig).toEqual(CALL_CONFIG);
        expect(state.clockMode).toBe("Simulated");
        expect(state.cplMode).toBe("Fast");
        expect(state.transmitConfig).toEqual({
            ...TRANSMIT_CONFIG,
            pushToTalkLabel: "A",
            pushToMuteLabel: null,
            radioPushToTalkLabel: null,
        });
        expect(state.radioConfig).toEqual({...RADIO_CONFIG, audioForVatsim: null});
        expect(state.playbackEnabled).toBe(true);
    });

    it("fetchSettings does not query the removed say again setting", async () => {
        invoke.mockImplementation((cmd: string) => Promise.resolve(RESPONSES[cmd]));

        await fetchSettings();

        expect(invoke.mock.calls.map(([cmd]) => cmd)).not.toContain("playback_get_say_again");
    });
});
