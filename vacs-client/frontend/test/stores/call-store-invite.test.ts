import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {startCall, useCallStore} from "../../src/stores/call-store.ts";
import {useAuthStore} from "../../src/stores/auth-store.ts";
import {CallTarget} from "../../src/types/call.ts";
import {CallId, ClientId, StationId} from "../../src/types/generic.ts";
import {makeTestCallDisplay} from "../util.ts";

const STATION_2: CallTarget = {station: "station2" as StationId};
const STATION_3: CallTarget = {station: "station3" as StationId};

afterEach(() => {
    useCallStore.getState().actions.reset();
    invoke.mockReset();
    invoke.mockImplementation(() => Promise.resolve(undefined));
});

describe("startCall conference add", () => {
    it("applies the invite optimistically", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted", {invitedTargets: []}),
            conferenceState: "modify",
        });

        await startCall(STATION_2);

        const display = useCallStore.getState().callDisplay;
        expect(display?.call.invitedTargets).toEqual([STATION_2]);
        expect(display?.call.ownInvitedTargets).toEqual([STATION_2]);
        expect(useCallStore.getState().conferenceState).toBe("active");
    });

    it("takes the invited target back when the invoke fails", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        const display = makeTestCallDisplay("accepted", {invitedTargets: []});
        useCallStore.setState({callDisplay: display, conferenceState: "modify"});
        invoke.mockImplementation(() => Promise.reject(new Error("offline")));

        await startCall(STATION_2);

        expect(useCallStore.getState().callDisplay).toEqual(display);
        expect(useCallStore.getState().conferenceState).toBe("modify");
    });

    it("does not resurrect a display that was reset while the invoke was pending", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted", {invitedTargets: []}),
            conferenceState: "modify",
        });
        invoke.mockImplementation(() => {
            useCallStore.getState().actions.reset();
            return Promise.reject(new Error("offline"));
        });

        await startCall(STATION_2);

        expect(useCallStore.getState().callDisplay).toBeUndefined();
        expect(useCallStore.getState().conferenceState).toBe("inactive");
    });

    it("keeps changes a call update made while the invoke was pending", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted", {invitedTargets: []}),
            conferenceState: "modify",
        });
        invoke.mockImplementation(() => {
            useCallStore.getState().actions.updateCall({
                callId: "call0" as CallId,
                invitedTargets: [STATION_2, STATION_3],
                joinedParticipants: {
                    ["client0" as ClientId]: {station: "station0" as StationId},
                    ["client1" as ClientId]: {station: "station1" as StationId},
                },
                conferenceLeader: "client1" as ClientId,
            });
            return Promise.reject(new Error("offline"));
        });

        await startCall(STATION_2);

        const display = useCallStore.getState().callDisplay;
        expect(display?.call.invitedTargets).toEqual([STATION_3]);
        expect(display?.call.isConferenceLeader).toBe(false);
    });
});
