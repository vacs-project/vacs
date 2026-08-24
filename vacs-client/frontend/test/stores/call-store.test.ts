import {afterEach, describe, expect, it} from "vitest";
import {CallDisplay, useCallStore} from "../../src/stores/call-store.ts";
import {CallTarget} from "../../src/types/call.ts";
import {CallId, ClientId, StationId} from "../../src/types/generic.ts";
import {useAuthStore} from "../../src/stores/auth-store.ts";
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

        it("keeps the conference active while two other parties remain", () => {
            useCallStore.setState({
                callDisplay: outgoingDisplay([STATION_1, STATION_2, STATION_3]),
                conferenceState: "active",
            });

            cancel(STATION_3);

            expect(useCallStore.getState().conferenceState).toBe("active");
        });

        it("deactivates the conference when one other party remains", () => {
            useCallStore.setState({
                callDisplay: outgoingDisplay([STATION_1, STATION_2]),
                conferenceState: "active",
            });

            cancel(STATION_2);

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

    describe("acceptIncomingCall", () => {
        it("seeds the accepting client into the joined roster", () => {
            useAuthStore.setState({cid: "client0" as ClientId});
            useCallStore.setState({
                incomingCalls: [
                    {
                        callId: CALL_ID,
                        source: {clientId: "client9" as ClientId},
                        target: {station: "station0" as StationId},
                        invitedTargets: [],
                        joinedParticipants: {
                            ["client9" as ClientId]: STATION_1,
                            ["client8" as ClientId]: STATION_2,
                        },
                        prio: false,
                    },
                ],
            });

            useCallStore.getState().actions.acceptIncomingCall(CALL_ID);

            const call = useCallStore.getState().callDisplay?.call;
            expect(call?.joinedParticipants["client0" as ClientId]).toEqual({
                target: {station: "station0" as StationId},
                state: "connected",
            });
            // Two other parties, so the display counts as a conference.
            expect(Object.keys(call?.joinedParticipants ?? {})).toHaveLength(3);
        });
    });

    describe("updateCall", () => {
        it("ignores updates for a terminal display", () => {
            const display = makeTestCallDisplay("error", {invitedTargets: []});
            useCallStore.setState({callDisplay: display});

            useCallStore.getState().actions.updateCall({
                callId: CALL_ID,
                invitedTargets: [STATION_1],
                joinedParticipants: {},
            });

            expect(useCallStore.getState().callDisplay).toBe(display);
        });

        const JOINED = {
            ["client0" as ClientId]: {station: "station0" as StationId},
            ["client1" as ClientId]: STATION_1,
        };

        it("clears rejected and errored annotations for re-invited targets", () => {
            const display = acceptedDisplay([]);
            useCallStore.setState({
                callDisplay: {
                    ...display,
                    rejectedTargets: [STATION_2],
                    erroredTargets: [{target: STATION_3, reason: "callFailure"}],
                },
            });

            useCallStore.getState().actions.updateCall({
                callId: CALL_ID,
                invitedTargets: [STATION_2, STATION_3],
                joinedParticipants: JOINED,
            });

            const next = useCallStore.getState().callDisplay;
            expect(next?.rejectedTargets).toEqual([]);
            expect(next?.erroredTargets).toEqual([]);
        });

        it("keeps an outgoing call alive when a target errors but another still rings", () => {
            useCallStore.setState({callDisplay: outgoingDisplay([STATION_1, STATION_2])});

            useCallStore.getState().actions.errorTargets({
                callId: CALL_ID,
                origin: {type: "targets", value: [STATION_2]},
                reason: "callFailure",
            });

            const display = useCallStore.getState().callDisplay;
            expect(display?.type).toBe("outgoing");
            expect(display?.call.invitedTargets).toEqual([STATION_1]);
            expect(display?.erroredTargets).toEqual([{target: STATION_2, reason: "callFailure"}]);
        });

        it("keeps annotations for targets that were not re-invited", () => {
            const display = acceptedDisplay([]);
            useCallStore.setState({
                callDisplay: {...display, rejectedTargets: [STATION_2]},
            });

            useCallStore.getState().actions.updateCall({
                callId: CALL_ID,
                invitedTargets: [STATION_3],
                joinedParticipants: JOINED,
            });

            expect(useCallStore.getState().callDisplay?.rejectedTargets).toEqual([STATION_2]);
        });
    });

    describe("updateCall conference leader", () => {
        function update(conferenceLeader: ClientId | null) {
            useCallStore.getState().actions.updateCall({
                callId: CALL_ID,
                invitedTargets: [STATION_3],
                joinedParticipants: {
                    ["client0" as ClientId]: {station: "station0" as StationId},
                    ["client1" as ClientId]: STATION_1,
                    ["client2" as ClientId]: STATION_2,
                },
                conferenceLeader,
            });
        }

        it("derives leadership when the update names this client", () => {
            useAuthStore.setState({cid: "client0" as ClientId});
            useCallStore.setState({callDisplay: acceptedDisplay([STATION_3])});

            update("client0" as ClientId);

            expect(useCallStore.getState().callDisplay?.call.isConferenceLeader).toBe(true);
        });

        it("derives non-leadership when the update names another client", () => {
            useAuthStore.setState({cid: "client0" as ClientId});
            useCallStore.setState({callDisplay: acceptedDisplay([STATION_3])});

            update("client1" as ClientId);

            expect(useCallStore.getState().callDisplay?.call.isConferenceLeader).toBe(false);
        });

        it("clears leadership when the update carries no leader", () => {
            useAuthStore.setState({cid: "client0" as ClientId});
            const display = acceptedDisplay([STATION_3]);
            useCallStore.setState({
                callDisplay: {
                    ...display,
                    call: {...display.call, isConferenceLeader: true},
                },
            });

            update(null);

            expect(useCallStore.getState().callDisplay?.call.isConferenceLeader).toBeUndefined();
        });
    });
});
