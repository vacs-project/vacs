import {afterEach, describe, expect, it, vi} from "vitest";
import {cleanup, render, screen} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import DirectAccessKeyButton from "../../../src/components/ui/DirectAccessKeyButton.tsx";
import {useProfileStore} from "../../../src/stores/profile-store.ts";
import {useSettingsStore} from "../../../src/stores/settings-store.ts";
import {ProfileId} from "../../../src/types/generic.ts";
import {Profile} from "../../../src/types/profile.ts";
import {RadioConfigWithLabels} from "../../../src/types/transmit.ts";

const TRACK_AUDIO: RadioConfigWithLabels = {
    integration: "TrackAudio",
    audioForVatsim: null,
    trackAudio: {endpoint: "ws://localhost:49080"},
};

function splitProfile(): Profile {
    return {
        id: "profile0" as ProfileId,
        view: "split",
        tabbed: [{label: ["A"], page: {rows: 1}}],
    };
}

afterEach(() => {
    useProfileStore.getState().reset();
    useSettingsStore.setState({radioConfig: undefined});
    vi.clearAllMocks();
    cleanup();
});

describe("DirectAccessKeyButton", () => {
    it("uses the narrower split-view width", () => {
        useProfileStore.getState().setProfile(splitProfile(), undefined);
        useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        render(<DirectAccessKeyButton color="gray">A</DirectAccessKeyButton>);

        const button = screen.getByRole("button", {name: "A"}) as HTMLButtonElement;
        expect(button.style.width).toBe("5.5rem");
    });

    it("uses the default width outside split view", () => {
        render(<DirectAccessKeyButton color="gray">A</DirectAccessKeyButton>);

        const button = screen.getByRole("button", {name: "A"}) as HTMLButtonElement;
        expect(button.style.width).toBe("6.25rem");
    });
});
