import {afterEach, describe, expect, it, vi} from "vitest";
import {act, renderHook} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import {usePageSync, useSplitView} from "../../src/hooks/page-hook.ts";
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

const AUDIO_FOR_VATSIM: RadioConfigWithLabels = {
    integration: "AudioForVatsim",
    audioForVatsim: {emit: null, emitLabel: null},
    trackAudio: null,
};

const NONE_CONFIG: RadioConfigWithLabels = {
    integration: null,
    audioForVatsim: null,
    trackAudio: null,
};

function tabbedProfile(view: Profile["view"]): Profile {
    return {
        id: "profile0" as ProfileId,
        view,
        tabbed: [{label: ["A"], page: {rows: 1}}],
    };
}

function geoProfile(view: Profile["view"]): Profile {
    return {
        id: "profile0" as ProfileId,
        view,
        geo: {direction: "row", children: []},
    };
}

afterEach(() => {
    void act(() => {
        useProfileStore.getState().reset();
        useNavigationStore.setState({
            page: "phone",
            menu: undefined,
            submenu: undefined,
            previous: {page: "phone", menu: undefined, submenu: undefined},
        });
        useSettingsStore.setState({radioConfig: undefined});
    });
    vi.clearAllMocks();
});

describe("useSplitView", () => {
    it("is true for a tabbed profile with view split and TrackAudio", () => {
        void act(() => {
            useProfileStore.getState().setProfile(tabbedProfile("split"), undefined);
            useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        });

        const {result} = renderHook(() => useSplitView());

        expect(result.current).toBe(true);
    });

    it("is true for a tabbed profile with view cycle and TrackAudio", () => {
        void act(() => {
            useProfileStore.getState().setProfile(tabbedProfile("cycle"), undefined);
            useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        });

        const {result} = renderHook(() => useSplitView());

        expect(result.current).toBe(true);
    });

    it("is false for a tabbed profile with view page", () => {
        void act(() => {
            useProfileStore.getState().setProfile(tabbedProfile("page"), undefined);
            useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        });

        const {result} = renderHook(() => useSplitView());

        expect(result.current).toBe(false);
    });

    it("is false for a geo profile even with view split and TrackAudio", () => {
        void act(() => {
            useProfileStore.getState().setProfile(geoProfile("split"), undefined);
            useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        });

        const {result} = renderHook(() => useSplitView());

        expect(result.current).toBe(false);
    });

    it("is false when the radio integration is AudioForVatsim", () => {
        void act(() => {
            useProfileStore.getState().setProfile(tabbedProfile("split"), undefined);
            useSettingsStore.setState({radioConfig: AUDIO_FOR_VATSIM});
        });

        const {result} = renderHook(() => useSplitView());

        expect(result.current).toBe(false);
    });

    it("is false when the radio integration is None", () => {
        void act(() => {
            useProfileStore.getState().setProfile(tabbedProfile("split"), undefined);
            useSettingsStore.setState({radioConfig: NONE_CONFIG});
        });

        const {result} = renderHook(() => useSplitView());

        expect(result.current).toBe(false);
    });

    it("is false when radioConfig has not loaded yet", () => {
        void act(() => {
            useProfileStore.getState().setProfile(tabbedProfile("split"), undefined);
        });

        const {result} = renderHook(() => useSplitView());

        expect(result.current).toBe(false);
    });
});

describe("usePageSync", () => {
    it("stays on phone until radioConfig arrives, then switches to split", () => {
        void act(() => {
            useProfileStore.getState().setProfile(tabbedProfile("split"), undefined);
        });
        renderHook(() => usePageSync());

        expect(useNavigationStore.getState().page).toBe("phone");

        void act(() => {
            useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        });

        expect(useNavigationStore.getState().page).toBe("split");
    });

    it("keeps the settings menu open while switching to TrackAudio opens split", () => {
        void act(() => {
            useProfileStore.getState().setProfile(tabbedProfile("split"), undefined);
            useNavigationStore.getState().openMenu("settings");
        });
        renderHook(() => usePageSync());

        void act(() => {
            useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        });

        expect(useNavigationStore.getState().page).toBe("split");
        expect(useNavigationStore.getState().menu).toBe("settings");
    });

    it("falls back to phone when TrackAudio is lost while on the split page", () => {
        void act(() => {
            useProfileStore.getState().setProfile(tabbedProfile("split"), undefined);
            useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        });
        renderHook(() => usePageSync());
        expect(useNavigationStore.getState().page).toBe("split");

        void act(() => {
            useSettingsStore.setState({radioConfig: AUDIO_FOR_VATSIM});
        });

        expect(useNavigationStore.getState().page).toBe("phone");
    });

    it("falls back to phone when a geo profile replaces a split profile", () => {
        void act(() => {
            useProfileStore.getState().setProfile(tabbedProfile("split"), undefined);
            useSettingsStore.setState({radioConfig: TRACK_AUDIO});
        });
        renderHook(() => usePageSync());
        expect(useNavigationStore.getState().page).toBe("split");

        void act(() => {
            useProfileStore.getState().setProfile(geoProfile("split"), undefined);
        });

        expect(useNavigationStore.getState().page).toBe("phone");
    });

    it("falls back to phone from the radio page without TrackAudio", () => {
        void act(() => {
            useNavigationStore.getState().setPage("radio");
        });

        renderHook(() => usePageSync());

        expect(useNavigationStore.getState().page).toBe("phone");
    });
});
