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
import {useConnectionStore} from "../../src/stores/connection-store.ts";
import {useErrorOverlayStore} from "../../src/stores/error-overlay-store.ts";
import {useStationsStore} from "../../src/stores/stations-store.ts";
import {CallTarget} from "../../src/types/call.ts";
import {CallId, ClientId, PositionId, StationId} from "../../src/types/generic.ts";
import {makeTestCallDisplay} from "../util.ts";

const STATION_2: CallTarget = {station: "station2" as StationId};
const STATION_3: CallTarget = {station: "station3" as StationId};

afterEach(() => {
    useCallStore.getState().actions.reset();
    useErrorOverlayStore.getState().close();
    useStationsStore.getState().reset();
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

describe("startCall guards", () => {
    it("refuses an add that would exceed the max conference size", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        // Two joined participants plus two new targets against a limit of three.
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted", {invitedTargets: []}),
            conferenceState: "modify",
            maxConferenceSize: 3,
        });

        await startCall(STATION_2, STATION_3);

        expect(invoke).not.toHaveBeenCalled();
        expect(useCallStore.getState().callDisplay?.call.invitedTargets).toEqual([]);
        const overlay = useErrorOverlayStore.getState();
        expect(overlay.visible).toBe(true);
        expect(overlay.title).toBe("Call");
        expect(overlay.detail).toBe("Max conference size of 3 exceeded.");
    });

    it("allows an add that exactly fills the max conference size", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted", {invitedTargets: []}),
            conferenceState: "modify",
            maxConferenceSize: 3,
        });

        await startCall(STATION_2);

        expect(invoke).toHaveBeenCalledWith("signaling_invite_to_call", expect.anything());
        expect(useErrorOverlayStore.getState().visible).toBe(false);
    });

    it("refuses an add by a client that is not the conference leader", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        const display = makeTestCallDisplay("accepted", {invitedTargets: []});
        useCallStore.setState({
            callDisplay: {...display, call: {...display.call, isConferenceLeader: false}},
            conferenceState: "modify",
        });

        await startCall(STATION_2);

        expect(invoke).not.toHaveBeenCalled();
        const overlay = useErrorOverlayStore.getState();
        expect(overlay.visible).toBe(true);
        expect(overlay.detail).toBe(
            "You are not the conference leader. Can not invite target to call.",
        );
    });

    it("refuses a second call while a display is up and the conference is not in modify", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        const display = makeTestCallDisplay("accepted", {invitedTargets: []});
        useCallStore.setState({callDisplay: display, conferenceState: "active"});

        await startCall(STATION_2);

        expect(invoke).not.toHaveBeenCalled();
        // Silently ignored, not reported: the user is simply already in a call.
        expect(useErrorOverlayStore.getState().visible).toBe(false);
        expect(useCallStore.getState().callDisplay).toBe(display);
    });

    it("consumes the temporary station source and clears it", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        useConnectionStore.getState().setConnectionInfo({
            displayName: "EDDF_TWR",
            positionId: "position0" as PositionId,
            frequency: "119.900",
        });
        useStationsStore.setState({
            defaultSource: "station0" as StationId,
            temporarySource: "station9" as StationId,
        });

        await startCall(STATION_2);

        expect(invoke).toHaveBeenCalledWith("signaling_invite_to_call", {
            source: {
                clientId: "client0" as ClientId,
                positionId: "position0" as PositionId,
                stationId: "station9" as StationId,
            },
            targets: [STATION_2],
            prio: false,
        });
        expect(useStationsStore.getState().temporarySource).toBeUndefined();
        expect(useStationsStore.getState().defaultSource).toBe("station0" as StationId);
    });

    it("falls back to the default station source and keeps it", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        useConnectionStore.getState().setConnectionInfo({
            displayName: "EDDF_TWR",
            positionId: "position0" as PositionId,
            frequency: "119.900",
        });
        useStationsStore.setState({defaultSource: "station0" as StationId});

        await startCall(STATION_2);

        expect(invoke).toHaveBeenCalledWith(
            "signaling_invite_to_call",
            expect.objectContaining({
                source: expect.objectContaining({stationId: "station0" as StationId}),
            }),
        );
        expect(useStationsStore.getState().defaultSource).toBe("station0" as StationId);
    });
});
