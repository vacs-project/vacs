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

import EndButton from "../../../src/components/ui/EndButton.tsx";
import {useCallStore} from "../../../src/stores/call-store.ts";
import {useNavigationStore} from "../../../src/stores/navigation-store.ts";
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
    useCallStore.getState().actions.reset();
    useProfileStore.getState().reset();
    useNavigationStore.setState({
        page: "phone",
        menu: undefined,
        submenu: undefined,
        previous: {page: "phone", menu: undefined, submenu: undefined},
    });
    useSettingsStore.setState({radioConfig: undefined});
    vi.clearAllMocks();
    cleanup();
});

describe("EndButton", () => {
    it("goes to the split page when split view is active", () => {
        useProfileStore.getState().setProfile(splitProfile(), undefined);
        useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        useNavigationStore.setState({page: "radio"});
        render(<EndButton />);

        fireEvent.click(screen.getByRole("button", {name: "END"}));

        expect(useNavigationStore.getState().page).toBe("split");
    });

    it("goes to the phone page without split view", () => {
        useNavigationStore.setState({page: "radio"});
        render(<EndButton />);

        fireEvent.click(screen.getByRole("button", {name: "END"}));

        expect(useNavigationStore.getState().page).toBe("phone");
    });
});
