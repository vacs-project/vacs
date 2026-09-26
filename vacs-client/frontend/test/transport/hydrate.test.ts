import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({
    invoke,
    listen,
    isTauri: false,
    isRemote: () => true,
}));

import {hydrateStores, SessionStateSnapshot} from "../../src/transport/hydrate.ts";
import {useProfileStore} from "../../src/stores/profile-store.ts";
import {ClientId, ProfileId} from "../../src/types/generic.ts";
import {Profile} from "../../src/types/profile.ts";

const PROFILE: Profile = {
    id: "profile0" as ProfileId,
    view: "page",
    tabbed: [{label: ["A"], page: {rows: 1}}],
};

function baseSnapshot(overrides: Partial<SessionStateSnapshot> = {}): SessionStateSnapshot {
    return {
        connectionState: "connected",
        sessionInfo: null,
        defaultCallSources: [],
        stations: [],
        clients: [],
        clientId: "client0" as ClientId,
        callConfig: {
            highlightIncomingCallTarget: true,
            enablePriorityCalls: true,
            enableCallStartSound: true,
            enableCallEndSound: true,
            useDefaultCallSources: true,
            forceRelay: false,
            enableParticipantJoinedSound: true,
            enableParticipantLeftSound: true,
        },
        clientPageSettings: {selected: undefined, configs: {}},
        capabilities: {
            alwaysOnTop: true,
            keybindListener: true,
            keybindEmitter: true,
            joystick: true,
            playback: true,
            platform: "Windows",
        },
        ...overrides,
    };
}

afterEach(() => {
    useProfileStore.getState().reset();
    vi.clearAllMocks();
});

describe("hydrateStores", () => {
    it("applies the profile and split profile width from a changed session profile", () => {
        const snapshot = baseSnapshot({
            sessionInfo: {
                client: {
                    id: "client0" as ClientId,
                    positionId: undefined,
                    displayName: "VIE_TWR",
                    frequency: "118.300",
                },
                profile: {type: "changed", activeProfile: {type: "specific", profile: PROFILE}},
                defaultCallSources: [],
                splitProfileWidth: 320,
            },
        });

        hydrateStores(snapshot);

        expect(useProfileStore.getState().profile).toEqual(PROFILE);
        expect(useProfileStore.getState().splitProfileWidth).toBe(320);
    });
});
