import {afterEach, beforeEach, describe, expect, it, vi} from "vitest";
import {renderHook} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({
    invoke,
    listen,
    isTauri: false,
    isRemote: () => true,
}));

import {useCallListStore, useLastDialledClientId} from "../../src/stores/call-list-store.ts";
import {startCall, useCallStore} from "../../src/stores/call-store.ts";
import {useAuthStore} from "../../src/stores/auth-store.ts";
import {useBlinkStore} from "../../src/stores/blink-store.ts";
import type {CallParticipants, CallTarget} from "../../src/types/call.ts";
import type {CallId, ClientId, StationId} from "../../src/types/generic.ts";
import {makeTestCallDisplay} from "../util.ts";

const CALL_ID = "call1" as CallId;
const OWN_CLIENT = "OWN1" as ClientId;
const CALLER = "CLR1" as ClientId;
const THIRD = "THD1" as ClientId;

const OWN_STATION: CallTarget = {station: "OWN_ST" as StationId};
const CALLER_STATION: CallTarget = {station: "CALLER_ST" as StationId};
const THIRD_STATION: CallTarget = {station: "THIRD_ST" as StationId};

const actions = () => useCallListStore.getState().actions;
const entry = (callId: CallId = CALL_ID) => useCallListStore.getState().callList.get(callId);

/** An incoming call ringing at us, already listed in the call list. */
function ringingIncomingCall(invitedTargets: CallTarget[], joined: CallParticipants) {
    actions().addIncomingCallListEntry({
        callId: CALL_ID,
        source: {clientId: CALLER, stationId: CALLER_STATION.station},
    });

    useCallStore.setState({
        incomingCalls: [
            {
                callId: CALL_ID,
                source: {clientId: CALLER, stationId: CALLER_STATION.station},
                target: OWN_STATION,
                invitedTargets,
                joinedParticipants: joined,
                prio: false,
            },
        ],
    });
}

beforeEach(() => {
    useAuthStore.setState({cid: OWN_CLIENT, status: "authenticated"});
});

afterEach(() => {
    useCallStore.getState().actions.reset();
    actions().clearCallList();
    useBlinkStore.getState().stopBlink();
    vi.clearAllMocks();
});

describe("call list entries", () => {
    it("lists an incoming call with its caller as the only target", () => {
        actions().addIncomingCallListEntry({
            callId: CALL_ID,
            source: {clientId: CALLER, stationId: CALLER_STATION.station},
        });

        expect(entry()).toMatchObject({
            type: "IN",
            name: "CALLER_ST",
            answered: undefined,
            targets: [{target: CALLER_STATION, clientId: CALLER}],
        });
    });

    it("names an outgoing call with several targets CONF", () => {
        actions().addOutgoingCallListEntry({
            callId: CALL_ID,
            targets: [CALLER_STATION, THIRD_STATION],
        });

        expect(entry()?.name).toBe("CONF");
        expect(entry()?.targets).toEqual([
            {target: CALLER_STATION, clientId: undefined},
            {target: THIRD_STATION, clientId: undefined},
        ]);
    });

    it("replaces the targets of an entry wholesale", () => {
        actions().addOutgoingCallListEntry({
            callId: CALL_ID,
            targets: [CALLER_STATION, THIRD_STATION],
        });
        actions().updateCallListEntry(CALL_ID, {
            targets: [{target: THIRD_STATION, clientId: THIRD}],
        });

        expect(entry()?.targets).toEqual([{target: THIRD_STATION, clientId: THIRD}]);
    });

    it("keeps the last roster when an update leaves no participants", () => {
        actions().addOutgoingCallListEntry({callId: CALL_ID, targets: [CALLER_STATION]});
        actions().updateCallListEntry(CALL_ID, {targets: []});

        expect(entry()?.targets).toEqual([{target: CALLER_STATION, clientId: undefined}]);
    });

    it("leaves answered untouched unless the update carries it", () => {
        actions().addOutgoingCallListEntry({callId: CALL_ID, targets: [CALLER_STATION]});

        actions().updateCallListEntry(CALL_ID, {
            targets: [{target: THIRD_STATION, clientId: THIRD}],
        });
        expect(entry()?.answered).toBeUndefined();

        actions().updateCallListEntry(CALL_ID, {answered: true});
        expect(entry()?.answered).toBe(true);
    });

    it("ignores updates for calls that are not listed", () => {
        actions().updateCallListEntry(CALL_ID, {answered: true});

        expect(useCallListStore.getState().callList.size).toBe(0);
    });
});

describe("call updates", () => {
    it("lists every participant of an incoming call except ourselves", () => {
        ringingIncomingCall([THIRD_STATION], {[CALLER]: CALLER_STATION});

        useCallStore.getState().actions.updateCall({
            callId: CALL_ID,
            invitedTargets: [THIRD_STATION],
            joinedParticipants: {[CALLER]: CALLER_STATION},
        });

        // Joined participants first, so the resolved client ids lead the Number column.
        expect(entry()?.targets).toEqual([
            {target: CALLER_STATION, clientId: CALLER},
            {target: THIRD_STATION, clientId: undefined},
        ]);
        expect(entry()?.name).toBe("CONF");
        expect(entry()?.answered).toBeUndefined();
    });

    it("lists a target that joined once, with its client id", () => {
        ringingIncomingCall([THIRD_STATION], {[CALLER]: CALLER_STATION});

        useCallStore.getState().actions.updateCall({
            callId: CALL_ID,
            invitedTargets: [THIRD_STATION],
            joinedParticipants: {[CALLER]: CALLER_STATION, [THIRD]: THIRD_STATION},
        });

        expect(entry()?.targets).toEqual([
            {target: CALLER_STATION, clientId: CALLER},
            {target: THIRD_STATION, clientId: THIRD},
        ]);
    });

    it("marks an outgoing call answered once someone joins", () => {
        actions().addOutgoingCallListEntry({callId: CALL_ID, targets: [CALLER_STATION]});
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("outgoing", {
                callId: CALL_ID,
                invitedTargets: [CALLER_STATION],
            }),
        });

        useCallStore.getState().actions.updateCall({
            callId: CALL_ID,
            invitedTargets: [],
            joinedParticipants: {[OWN_CLIENT]: OWN_STATION, [CALLER]: CALLER_STATION},
        });

        expect(entry()?.answered).toBe(true);
        expect(entry()?.targets).toEqual([{target: CALLER_STATION, clientId: CALLER}]);
    });

    it("does not mark an unanswered outgoing call answered", () => {
        actions().addOutgoingCallListEntry({
            callId: CALL_ID,
            targets: [CALLER_STATION, THIRD_STATION],
        });
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("outgoing", {
                callId: CALL_ID,
                invitedTargets: [CALLER_STATION, THIRD_STATION],
            }),
        });

        useCallStore.getState().actions.updateCall({
            callId: CALL_ID,
            invitedTargets: [CALLER_STATION],
            joinedParticipants: {},
        });

        expect(entry()?.answered).toBeUndefined();
    });
});

describe("conference add", () => {
    beforeEach(() => {
        actions().addIncomingCallListEntry({
            callId: CALL_ID,
            source: {clientId: CALLER, stationId: CALLER_STATION.station},
        });

        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted", {callId: CALL_ID, invitedTargets: []}),
            conferenceState: "modify",
        });

        // The backend reuses the call id of the current call for a conference add.
        invoke.mockResolvedValueOnce(CALL_ID);
    });

    it("extends the existing entry instead of replacing it", async () => {
        const time = entry()!.time;

        await startCall(THIRD_STATION);

        expect(invoke).toHaveBeenCalledWith("signaling_invite_to_call", expect.anything());
        expect(entry()).toMatchObject({
            type: "IN",
            time,
            name: "CONF",
            targets: [
                {target: CALLER_STATION, clientId: CALLER},
                {target: THIRD_STATION, clientId: undefined},
            ],
        });
    });

    it("does not list a target the entry already holds twice", async () => {
        await startCall(CALLER_STATION);

        expect(entry()?.targets).toEqual([{target: CALLER_STATION, clientId: CALLER}]);
        expect(entry()?.name).toBe("CALLER_ST");
    });
});

describe("rejected calls", () => {
    beforeEach(() => {
        actions().addOutgoingCallListEntry({
            callId: CALL_ID,
            targets: [CALLER_STATION, THIRD_STATION],
        });

        useCallStore.setState({
            callDisplay: makeTestCallDisplay("outgoing", {
                callId: CALL_ID,
                invitedTargets: [CALLER_STATION, THIRD_STATION],
            }),
        });
    });

    it("stays unmarked while other targets are still ringing", () => {
        useCallStore.getState().actions.rejectTargets(CALL_ID, [CALLER_STATION]);

        expect(entry()?.answered).toBeUndefined();
    });

    it("is unanswered once the last target rejected", () => {
        useCallStore.getState().actions.rejectTargets(CALL_ID, [CALLER_STATION]);
        useCallStore.getState().actions.rejectTargets(CALL_ID, [THIRD_STATION]);

        expect(entry()?.answered).toBe(false);
    });
});

describe("useLastDialledClientId", () => {
    const redial = () => renderHook(() => useLastDialledClientId()).result.current;

    it("returns the client of the most recent outgoing call", () => {
        actions().addOutgoingCallListEntry({
            callId: "old" as CallId,
            targets: [{client: THIRD}],
        });
        actions().addOutgoingCallListEntry({
            callId: "new" as CallId,
            targets: [{client: CALLER}],
        });

        expect(redial()).toBe(CALLER);
    });

    it("skips incoming calls", () => {
        actions().addOutgoingCallListEntry({
            callId: "out" as CallId,
            targets: [{client: CALLER}],
        });
        actions().addIncomingCallListEntry({
            callId: "in" as CallId,
            source: {clientId: THIRD},
        });

        expect(redial()).toBe(CALLER);
    });

    it("skips conferences, which have no single client to redial", () => {
        actions().addOutgoingCallListEntry({
            callId: "single" as CallId,
            targets: [{client: CALLER}],
        });
        actions().addOutgoingCallListEntry({
            callId: "conf" as CallId,
            targets: [{client: THIRD}, THIRD_STATION],
        });

        expect(redial()).toBe(CALLER);
    });

    it("skips a station dial, which is not redialled by client id", () => {
        actions().addOutgoingCallListEntry({callId: CALL_ID, targets: [CALLER_STATION]});
        actions().updateCallListEntry(CALL_ID, {
            targets: [{target: CALLER_STATION, clientId: CALLER}],
        });

        expect(redial()).toBeUndefined();
    });
});
