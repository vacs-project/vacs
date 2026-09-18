import {afterEach, describe, expect, it, vi} from "vitest";
import {cleanup, fireEvent, render, screen} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import RadioButton from "../../../src/components/ui/RadioButton.tsx";
import {useNavigationStore} from "../../../src/stores/navigation-store.ts";
import {useRadioStore} from "../../../src/stores/radio-store.ts";
import {useSettingsStore} from "../../../src/stores/settings-store.ts";
import {RadioConfigWithLabels} from "../../../src/types/transmit.ts";

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

function getButton() {
    return screen.getByRole("button", {name: "Radio"}) as HTMLButtonElement;
}

afterEach(() => {
    useNavigationStore.setState({
        page: "phone",
        menu: undefined,
        submenu: undefined,
        previous: {page: "phone", menu: undefined, submenu: undefined},
    });
    useRadioStore.setState({radioState: undefined});
    useSettingsStore.setState({radioConfig: undefined});
    vi.clearAllMocks();
    cleanup();
});

describe("RadioButton", () => {
    it("goes to the radio page and reconnects when TrackAudio is disconnected", () => {
        useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        useRadioStore.setState({radioState: {state: "Disconnected"}});
        render(<RadioButton />);

        fireEvent.click(getButton());

        expect(useNavigationStore.getState().page).toBe("radio");
        expect(invoke).toHaveBeenCalledWith("radio_reconnect", undefined);
    });

    it("leaves the page unchanged for a non-TrackAudio integration", () => {
        useSettingsStore.setState({radioConfig: AUDIO_FOR_VATSIM});
        useRadioStore.setState({radioState: {state: "Disconnected"}});
        render(<RadioButton />);

        fireEvent.click(getButton());

        expect(useNavigationStore.getState().page).toBe("phone");
        expect(invoke).toHaveBeenCalledWith("radio_reconnect", undefined);
    });

    it("is disabled when the radio is not configured", () => {
        useRadioStore.setState({radioState: {state: "NotConfigured"}});
        render(<RadioButton />);

        expect(getButton().disabled).toBe(true);
    });

    it("is enabled while disconnected", () => {
        useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        useRadioStore.setState({radioState: {state: "Disconnected"}});
        render(<RadioButton />);

        expect(getButton().disabled).toBe(false);
    });
});
