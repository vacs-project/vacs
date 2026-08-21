import {afterEach, describe, expect, it} from "vitest";
import {CallDisplay, useCallStore} from "../../src/stores/call-store.ts";
import {CallTarget} from "../../src/types/call.ts";
import {CallId, ClientId, StationId} from "../../src/types/generic.ts";
import {makeTestCallDisplay} from "../util.ts";

const CALL_ID = "call0" as CallId;
const OTHER_CALL_ID = "call1" as CallId;

const STATION_1: CallTarget = {station: "station1" as StationId};
const STATION_2: CallTarget = {station: "station2" as StationId};
const STATION_3: CallTarget = {station: "station3" as StationId};

function outgoingDisplay(
    invitedTargets: CallTarget[],
    prioTargets: CallTarget[] = [],
): CallDisplay {
    const display = makeTestCallDisplay("outgoing", {invitedTargets}, prioTargets);
    return {...display, call: {...display.call, ownInvitedTargets: invitedTargets}};
}

// An accepted conference: this client (client0/station0) plus client1/station1
// joined, with further targets still ringing.
function acceptedDisplay(
    invitedTargets: CallTarget[],
    prioTargets: CallTarget[] = [],
): CallDisplay {
    const display = makeTestCallDisplay("accepted", {invitedTargets}, prioTargets);
    return {...display, call: {...display.call, ownInvitedTargets: invitedTargets}};
}

function cancel(target: CallTarget, callId: CallId = CALL_ID) {
    useCallStore.getState().actions.cancelInvitedTarget(callId, target);
}

afterEach(() => {
    useCallStore.getState().actions.reset();
});

describe("call store", () => {
    describe("cancelInvitedTarget", () => {
        it("removes only the cancelled target from invitedTargets", () => {
            useCallStore.setState({
                callDisplay: outgoingDisplay([STATION_1, STATION_2, STATION_3]),
            });

            cancel(STATION_2);

            expect(useCallStore.getState().callDisplay?.call.invitedTargets).toEqual([
                STATION_1,
                STATION_3,
            ]);
        });

        it("keeps the cancelled target in ownInvitedTargets", () => {
            useCallStore.setState({callDisplay: outgoingDisplay([STATION_1, STATION_2])});

            cancel(STATION_2);

            expect(useCallStore.getState().callDisplay?.call.ownInvitedTargets).toEqual([
                STATION_1,
                STATION_2,
            ]);
        });

        it("replaces the call display instead of mutating the previous one", () => {
            useCallStore.setState({callDisplay: outgoingDisplay([STATION_1, STATION_2])});
            const before = useCallStore.getState().callDisplay!;

            cancel(STATION_2);

            const after = useCallStore.getState().callDisplay!;
            expect(after).not.toBe(before);
            expect(before.call.invitedTargets).toEqual([STATION_1, STATION_2]);
        });

        it("drops the prio flag of the cancelled target and keeps it for the rest", () => {
            useCallStore.setState({
                callDisplay: acceptedDisplay(
                    [STATION_2, STATION_3],
                    [STATION_1, STATION_2, STATION_3],
                ),
            });

            cancel(STATION_3);

            // station1 is a joined participant, station2 is still ringing.
            expect(useCallStore.getState().callDisplay?.prioTargets).toEqual([
                STATION_1,
                STATION_2,
            ]);
        });

        it("deactivates the conference when two members remain", () => {
            useCallStore.setState({
                callDisplay: outgoingDisplay([STATION_1, STATION_2, STATION_3]),
                conferenceState: "active",
            });

            cancel(STATION_3);

            expect(useCallStore.getState().conferenceState).toBe("inactive");
        });

        it("counts joined participants toward the remaining call size", () => {
            // Two joined participants plus one ringing target: cancelling the
            // ringing one leaves a 1:1 call.
            useCallStore.setState({
                callDisplay: acceptedDisplay([STATION_3]),
                conferenceState: "active",
            });

            cancel(STATION_3);

            expect(useCallStore.getState().conferenceState).toBe("inactive");
            expect(useCallStore.getState().callDisplay?.call.invitedTargets).toEqual([]);
            expect(
                Object.keys(useCallStore.getState().callDisplay!.call.joinedParticipants),
            ).toEqual(["client0" as ClientId, "client1" as ClientId]);
        });

        it("keeps the conference state while more than two members remain", () => {
            useCallStore.setState({
                callDisplay: acceptedDisplay([STATION_2, STATION_3]),
                conferenceState: "modify",
            });

            cancel(STATION_3);

            expect(useCallStore.getState().conferenceState).toBe("modify");
        });

        it("ignores a cancellation for a different call", () => {
            const display = outgoingDisplay([STATION_1, STATION_2]);
            useCallStore.setState({callDisplay: display, conferenceState: "active"});

            cancel(STATION_2, OTHER_CALL_ID);

            expect(useCallStore.getState().callDisplay).toBe(display);
            expect(useCallStore.getState().conferenceState).toBe("active");
        });

        it("ignores a cancellation while no call is displayed", () => {
            cancel(STATION_2);

            expect(useCallStore.getState().callDisplay).toBeUndefined();
        });
    });
});
