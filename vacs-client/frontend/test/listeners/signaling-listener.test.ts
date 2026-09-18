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

import {setupSignalingListeners} from "../../src/listeners/signaling-listener.ts";
import {useNavigationStore} from "../../src/stores/navigation-store.ts";
import {useProfileStore} from "../../src/stores/profile-store.ts";
import {ClientId, ProfileId} from "../../src/types/generic.ts";
import {Profile} from "../../src/types/profile.ts";

const PROFILE: Profile = {
    id: "profile0" as ProfileId,
    view: "page",
    tabbed: [{label: ["A"], page: {rows: 1}}],
};

const CLIENT_INFO = {
    id: "client0" as ClientId,
    positionId: undefined,
    displayName: "VIE_TWR",
    frequency: "118.300",
};

function findCallback(event: string) {
    const call = listen.mock.calls.find(([e]) => e === event);
    if (call === undefined) throw new Error(`${event} was never registered`);
    return call[1];
}

afterEach(() => {
    useProfileStore.getState().reset();
    useNavigationStore.setState({
        page: "phone",
        menu: undefined,
        submenu: undefined,
        previous: {page: "phone", menu: undefined, submenu: undefined},
    });
    vi.clearAllMocks();
});

describe("setupSignalingListeners", () => {
    describe("signaling:connected", () => {
        it("applies the split profile width from the payload", () => {
            const teardown = setupSignalingListeners();

            findCallback("signaling:connected")({
                payload: {
                    client: CLIENT_INFO,
                    profile: {type: "changed", activeProfile: {type: "specific", profile: PROFILE}},
                    defaultCallSources: [],
                    splitProfileWidth: 320,
                },
            });

            expect(useProfileStore.getState().profile).toEqual(PROFILE);
            expect(useProfileStore.getState().splitProfileWidth).toBe(320);

            teardown();
        });

        it("clears the split profile width when the payload omits it", () => {
            useProfileStore.getState().setProfile(PROFILE, 320);
            const teardown = setupSignalingListeners();

            findCallback("signaling:connected")({
                payload: {
                    client: CLIENT_INFO,
                    profile: {type: "changed", activeProfile: {type: "specific", profile: PROFILE}},
                    defaultCallSources: [],
                    splitProfileWidth: undefined,
                },
            });

            expect(useProfileStore.getState().splitProfileWidth).toBeUndefined();

            teardown();
        });
    });

    describe("signaling:disconnected", () => {
        it("returns to the phone page and resets the profile store", () => {
            useNavigationStore.getState().setPage("split");
            useProfileStore.getState().setProfile(PROFILE, 320);
            const teardown = setupSignalingListeners();

            findCallback("signaling:disconnected")({payload: undefined});

            expect(useNavigationStore.getState().page).toBe("phone");
            expect(useProfileStore.getState().profile).toBeUndefined();
            expect(useProfileStore.getState().splitProfileWidth).toBeUndefined();

            teardown();
        });
    });

    describe("signaling:test-profile", () => {
        it("closes the menu and keeps the current page", () => {
            useNavigationStore.getState().openMenu("settings");
            const teardown = setupSignalingListeners();

            findCallback("signaling:test-profile")({payload: PROFILE});

            expect(useNavigationStore.getState().menu).toBeUndefined();
            expect(useNavigationStore.getState().page).toBe("phone");
            expect(useProfileStore.getState().profile).toEqual(PROFILE);

            teardown();
        });
    });
});
