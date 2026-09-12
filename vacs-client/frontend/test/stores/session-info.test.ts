import {afterEach, describe, expect, it} from "vitest";
import {applySessionInfo} from "../../src/stores/session-info.ts";
import {useCallStore} from "../../src/stores/call-store.ts";
import {useConnectionStore} from "../../src/stores/connection-store.ts";
import {useProfileStore} from "../../src/stores/profile-store.ts";
import {useStationsStore} from "../../src/stores/stations-store.ts";
import type {SessionInfo} from "../../src/types/client.ts";
import type {Profile} from "../../src/types/profile.ts";
import type {ClientId, PositionId, ProfileId, StationId} from "../../src/types/generic.ts";

const CLIENT: SessionInfo["client"] = {
    id: "client0" as ClientId,
    positionId: "LOVV_CTR" as PositionId,
    displayName: "LOVV_N_CTR",
    frequency: "134.350",
};

const PROFILE: Profile = {id: "profile0" as ProfileId, tabbed: []};

const sessionInfo = (overrides: Partial<SessionInfo> = {}): SessionInfo => ({
    client: CLIENT,
    profile: {type: "unchanged"},
    defaultCallSources: [],
    ...overrides,
});

afterEach(() => {
    useCallStore.getState().actions.reset();
    useConnectionStore.setState({info: {displayName: "", positionId: undefined, frequency: ""}});
    useProfileStore.getState().reset();
    useStationsStore.getState().reset();
});

describe("applySessionInfo", () => {
    it("applies the client info to the connection store", () => {
        applySessionInfo(sessionInfo());

        expect(useConnectionStore.getState().info).toEqual({
            id: "client0" as ClientId,
            positionId: "LOVV_CTR" as PositionId,
            displayName: "LOVV_N_CTR",
            frequency: "134.350",
        });
    });

    it("sets the profile when the session reports a change", () => {
        applySessionInfo(
            sessionInfo({
                profile: {type: "changed", activeProfile: {type: "specific", profile: PROFILE}},
            }),
        );

        expect(useProfileStore.getState().profile).toEqual(PROFILE);
    });

    it("keeps the current profile when the session reports it unchanged", () => {
        useProfileStore.getState().setProfile(PROFILE);

        applySessionInfo(
            sessionInfo({
                profile: {
                    type: "unchanged",
                    activeProfile: {
                        type: "specific",
                        profile: {id: "profile9" as ProfileId, tabbed: []},
                    },
                },
            }),
        );

        expect(useProfileStore.getState().profile).toEqual(PROFILE);
    });

    it("keeps the current profile when a change carries no profile", () => {
        useProfileStore.getState().setProfile(PROFILE);

        applySessionInfo(sessionInfo({profile: {type: "changed", activeProfile: {type: "none"}}}));

        expect(useProfileStore.getState().profile).toEqual(PROFILE);
    });

    it("picks the first own station from the position default sources", () => {
        useStationsStore.getState().setStations([
            {id: "LOVV_N1" as StationId, own: false},
            {id: "LOVV_N2" as StationId, own: true},
        ]);

        applySessionInfo(
            sessionInfo({
                defaultCallSources: ["LOVV_N1" as StationId, "LOVV_N2" as StationId],
            }),
        );

        expect(useStationsStore.getState().positionDefaultSources).toEqual([
            "LOVV_N1" as StationId,
            "LOVV_N2" as StationId,
        ]);
        expect(useStationsStore.getState().defaultSource).toBe("LOVV_N2" as StationId);
    });

    it("sets the max conference size the server reports", () => {
        applySessionInfo(sessionInfo({maxConfSize: 6}));

        expect(useCallStore.getState().maxConferenceSize).toBe(6);
    });

    it("clears the max conference size when the server omits it", () => {
        useCallStore.getState().actions.setMaxConferenceSize(6);

        applySessionInfo(sessionInfo());

        expect(useCallStore.getState().maxConferenceSize).toBeUndefined();
    });
});
