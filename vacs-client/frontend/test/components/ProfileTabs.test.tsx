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

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import ProfileTabs from "../../src/components/ProfileTabs.tsx";
import {useNavigationStore} from "../../src/stores/navigation-store.ts";
import {useProfileStore} from "../../src/stores/profile-store.ts";
import {useSettingsStore} from "../../src/stores/settings-store.ts";
import {ProfileId} from "../../src/types/generic.ts";
import {Profile} from "../../src/types/profile.ts";
import {RadioConfigWithLabels} from "../../src/types/transmit.ts";

const TRACK_AUDIO: RadioConfigWithLabels = {
    integration: "TrackAudio",
    audioForVatsim: null,
    trackAudio: {endpoint: "ws://localhost:49080"},
};

function tabbedProfile(view: Profile["view"]): Profile {
    return {
        id: "profile0" as ProfileId,
        view,
        tabbed: [
            {label: ["A"], page: {rows: 1}},
            {label: ["B"], page: {rows: 1}},
        ],
    };
}

afterEach(() => {
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

describe("ProfileTabs", () => {
    it("goes to the phone page when selecting a tab from the radio page without split view", () => {
        useProfileStore.getState().setProfile(tabbedProfile("page"), undefined);
        useNavigationStore.setState({page: "radio"});
        render(<ProfileTabs />);

        fireEvent.click(screen.getByRole("button", {name: "B"}));

        expect(useNavigationStore.getState().page).toBe("phone");
    });

    it("goes to the split page when selecting a tab from the radio page in split view", () => {
        useProfileStore.getState().setProfile(tabbedProfile("split"), undefined);
        useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        useNavigationStore.setState({page: "radio"});
        render(<ProfileTabs />);

        fireEvent.click(screen.getByRole("button", {name: "B"}));

        expect(useNavigationStore.getState().page).toBe("split");
    });

    it("closes the settings menu and keeps the page on tab click", () => {
        useProfileStore.getState().setProfile(tabbedProfile("page"), undefined);
        useNavigationStore.setState({page: "phone"});
        useNavigationStore.getState().openMenu("settings");
        render(<ProfileTabs />);

        fireEvent.click(screen.getByRole("button", {name: "B"}));

        expect(useNavigationStore.getState().menu).toBeUndefined();
        expect(useNavigationStore.getState().page).toBe("phone");
    });
});
