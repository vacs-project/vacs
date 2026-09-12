import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {OutgoingCallEvent, startCall, useCallStore} from "../../src/stores/call-store.ts";
import {useCallListStore} from "../../src/stores/call-list-store.ts";
import {useBlinkStore} from "../../src/stores/blink-store.ts";
import {useAuthStore} from "../../src/stores/auth-store.ts";
import {CallTarget} from "../../src/types/call.ts";
import {CallId, ClientId, StationId} from "../../src/types/generic.ts";
import {makeTestCallDisplay} from "../util.ts";

const CALL_ID = "call0" as CallId;
const STATION_1: CallTarget = {station: "station1" as StationId};
const STATION_2: CallTarget = {station: "station2" as StationId};
const STATION_3: CallTarget = {station: "station3" as StationId};

const outgoing = (overrides: Partial<OutgoingCallEvent> = {}): OutgoingCallEvent => ({
    callId: CALL_ID,
    source: {clientId: "client0" as ClientId},
    targets: [STATION_2],
    prio: false,
    ...overrides,
});

const actions = () => useCallStore.getState().actions;

afterEach(() => {
    useCallStore.getState().actions.reset();
    useCallListStore.setState({callList: new Map()});
    useBlinkStore.getState().stopBlink();
    invoke.mockReset();
    invoke.mockImplementation(() => Promise.resolve(undefined));
});

describe("outgoing call event", () => {
    it("creates the outgoing display and the call list entry", () => {
        actions().applyOutgoingCall(outgoing());

        const display = useCallStore.getState().callDisplay;
        expect(display?.type).toBe("outgoing");
        expect(display?.call.callId).toBe(CALL_ID);
        expect(display?.call.invitedTargets).toEqual([STATION_2]);
        expect(display?.call.ownInvitedTargets).toEqual([STATION_2]);
        expect(useCallListStore.getState().callList.get(CALL_ID)?.type).toBe("OUT");
    });

    it("marks a fresh priority call's targets and starts blinking", () => {
        actions().applyOutgoingCall(outgoing({prio: true}));

        expect(useCallStore.getState().callDisplay?.prioTargets).toEqual([STATION_2]);
        expect(useBlinkStore.getState().blinkTimeoutId).toBeDefined();
    });

    it("leaves the display of a fresh call to the event", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        invoke.mockResolvedValueOnce(CALL_ID);

        await startCall(STATION_2);

        expect(invoke).toHaveBeenCalledWith("signaling_invite_to_call", expect.anything());
        expect(useCallStore.getState().callDisplay).toBeUndefined();
    });

    it("keeps a rejection that arrives before the invoke resolves", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        let resolveInvoke: (callId: CallId) => void = () => {};
        invoke.mockImplementationOnce(() => new Promise(resolve => (resolveInvoke = resolve)));

        const pending = startCall(STATION_2);
        // Backend order: the outgoing call, then the instant rejection, then the reply.
        actions().applyOutgoingCall(outgoing());
        actions().rejectTargets(CALL_ID, [STATION_2]);
        resolveInvoke(CALL_ID);
        await pending;

        const display = useCallStore.getState().callDisplay;
        expect(display?.type).toBe("rejected");
        expect(display?.rejectedTargets).toEqual([STATION_2]);
    });

    it("is idempotent after the optimistic conference add", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted", {callId: CALL_ID, invitedTargets: []}),
            conferenceState: "modify",
        });
        invoke.mockResolvedValueOnce(CALL_ID);

        await startCall(STATION_2);
        actions().applyOutgoingCall(outgoing());

        const display = useCallStore.getState().callDisplay;
        expect(display?.type).toBe("accepted");
        expect(display?.call.invitedTargets).toEqual([STATION_2]);
        expect(display?.call.ownInvitedTargets).toEqual([STATION_2]);
        expect(display?.call.isConferenceLeader).toBe(true);
        expect(useCallStore.getState().conferenceState).toBe("active");
    });

    it("adds the invited targets on an instance that did not initiate the add", () => {
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted", {callId: CALL_ID, invitedTargets: []}),
            conferenceState: "active",
        });

        actions().applyOutgoingCall(outgoing({prio: true}));

        const display = useCallStore.getState().callDisplay;
        expect(display?.call.invitedTargets).toEqual([STATION_2]);
        expect(display?.call.ownInvitedTargets).toEqual([STATION_2]);
        expect(display?.prioTargets).toEqual([STATION_2]);
    });

    it("ignores the event for a terminal display of the same call", () => {
        const rejected = makeTestCallDisplay("rejected", {callId: CALL_ID, invitedTargets: []});
        useCallStore.setState({callDisplay: rejected});

        actions().applyOutgoingCall(outgoing());

        expect(useCallStore.getState().callDisplay).toEqual(rejected);
    });

    it("replaces the display of another call", () => {
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("rejected", {callId: "old" as CallId}),
        });

        actions().applyOutgoingCall(outgoing());

        const display = useCallStore.getState().callDisplay;
        expect(display?.type).toBe("outgoing");
        expect(display?.call.callId).toBe(CALL_ID);
    });
});

describe("outgoing call event target bookkeeping", () => {
    it("ignores an event without targets", () => {
        actions().applyOutgoingCall(outgoing({targets: []}));

        expect(useCallStore.getState().callDisplay).toBeUndefined();
        expect(useCallListStore.getState().callList.size).toBe(0);
    });

    it("does not duplicate prio targets when the event is re-delivered", () => {
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted", {callId: CALL_ID, invitedTargets: []}),
            conferenceState: "active",
        });

        actions().applyOutgoingCall(outgoing({prio: true}));
        actions().applyOutgoingCall(outgoing({prio: true}));

        const display = useCallStore.getState().callDisplay;
        expect(display?.call.invitedTargets).toEqual([STATION_2]);
        expect(display?.call.ownInvitedTargets).toEqual([STATION_2]);
        expect(display?.prioTargets).toEqual([STATION_2]);
    });

    it("clears rejected and errored annotations of re-invited targets", () => {
        const display = makeTestCallDisplay("accepted", {callId: CALL_ID, invitedTargets: []});
        useCallStore.setState({
            callDisplay: {
                ...display,
                rejectedTargets: [STATION_2],
                erroredTargets: [
                    {target: STATION_3, reason: "callFailure"},
                    {target: STATION_1, reason: "autoHangup"},
                ],
            },
        });

        actions().applyOutgoingCall(outgoing({targets: [STATION_2, STATION_3]}));

        const next = useCallStore.getState().callDisplay;
        expect(next?.rejectedTargets).toEqual([]);
        // The annotation of a target that was not re-invited survives.
        expect(next?.erroredTargets).toEqual([{target: STATION_1, reason: "autoHangup"}]);
    });
});

describe("rollBackInvite", () => {
    it("restores the annotations of the rolled back targets", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        const display = makeTestCallDisplay("accepted", {callId: CALL_ID, invitedTargets: []});
        useCallStore.setState({
            callDisplay: {
                ...display,
                rejectedTargets: [STATION_2],
                erroredTargets: [{target: STATION_3, reason: "callFailure"}],
            },
            conferenceState: "modify",
        });
        invoke.mockImplementation(() => Promise.reject(new Error("offline")));

        await startCall(STATION_2, STATION_3);

        const next = useCallStore.getState().callDisplay;
        expect(next?.call.invitedTargets).toEqual([]);
        expect(next?.call.ownInvitedTargets).toEqual([]);
        expect(next?.rejectedTargets).toEqual([STATION_2]);
        expect(next?.erroredTargets).toEqual([{target: STATION_3, reason: "callFailure"}]);
        expect(useCallStore.getState().conferenceState).toBe("modify");
    });

    it("removes the rolled back targets from the call list entry", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        useCallListStore
            .getState()
            .actions.addOutgoingCallListEntry({callId: CALL_ID, targets: [STATION_1]});
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted", {callId: CALL_ID, invitedTargets: []}),
            conferenceState: "modify",
        });
        invoke.mockImplementation(() => Promise.reject(new Error("offline")));

        await startCall(STATION_2);

        expect(useCallListStore.getState().callList.get(CALL_ID)?.targets).toEqual([
            {target: STATION_1, clientId: undefined},
        ]);
    });

    it("gives up the optimistic leadership claim", async () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted", {callId: CALL_ID, invitedTargets: []}),
            conferenceState: "modify",
        });
        invoke.mockImplementation(() => Promise.reject(new Error("offline")));

        await startCall(STATION_2);

        expect(useCallStore.getState().callDisplay?.call.isConferenceLeader).toBeUndefined();
    });
});
