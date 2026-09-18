import {afterEach, describe, expect, it, vi} from "vitest";
import {cleanup, fireEvent, render, screen, waitFor} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import RadioPage from "../../src/pages/RadioPage.tsx";
import {useRadioStore} from "../../src/stores/radio-store.ts";
import {useSettingsStore} from "../../src/stores/settings-store.ts";
import {RadioStation} from "../../src/types/radio.ts";
import {RadioConfigWithLabels} from "../../src/types/transmit.ts";

const TRACK_AUDIO: RadioConfigWithLabels = {
    integration: "TrackAudio",
    audioForVatsim: null,
    trackAudio: {endpoint: "ws://localhost:49080"},
};

const AUDIO_FOR_VATSIM: RadioConfigWithLabels = {
    integration: "AudioForVatsim",
    audioForVatsim: {emit: null, emitLabel: null},
    trackAudio: null,
};

const STATION: RadioStation = {
    callsign: "VIE_TWR",
    frequency: 118_300_000,
    rx: true,
    tx: false,
    xc: false,
    xca: false,
    headset: false,
    output_muted: false,
    is_available: true,
};

afterEach(() => {
    useRadioStore.setState({radioState: undefined});
    useSettingsStore.setState({radioConfig: undefined});
    vi.clearAllMocks();
    cleanup();
});

describe("RadioPage", () => {
    it("renders nothing and never fetches stations without TrackAudio", async () => {
        useSettingsStore.setState({radioConfig: AUDIO_FOR_VATSIM});
        useRadioStore.setState({radioState: {state: "Connected"}});
        const {container} = render(<RadioPage />);

        expect(container.innerHTML).toBe("");
        expect(invoke.mock.calls.map(call => call[0])).not.toContain("radio_get_stations");
    });

    it("shows a placeholder and retries when disconnected", () => {
        useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        useRadioStore.setState({radioState: {state: "Disconnected"}});
        render(<RadioPage />);

        expect(screen.getByText("No TrackAudio radio connection.")).toBeDefined();

        fireEvent.click(screen.getByText("Retry"));

        expect(invoke).toHaveBeenCalledWith("radio_reconnect", undefined);
    });

    it("shows a placeholder when not configured", () => {
        useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        useRadioStore.setState({radioState: {state: "NotConfigured"}});
        render(<RadioPage />);

        expect(screen.getByText("No TrackAudio radio connection.")).toBeDefined();
    });

    it("shows a connection-failed message on error", () => {
        useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        useRadioStore.setState({radioState: {state: "Error"}});
        render(<RadioPage />);

        expect(screen.getByText("TrackAudio radio connection failed.")).toBeDefined();
    });

    it("renders the station list when connected", async () => {
        useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        useRadioStore.setState({radioState: {state: "Connected"}});
        invoke.mockImplementation((cmd: string) =>
            cmd === "radio_get_stations" ? Promise.resolve([STATION]) : Promise.resolve(undefined),
        );
        render(<RadioPage />);

        await waitFor(() => expect(screen.getByText("VIE_TWR")).toBeDefined());
    });
});
