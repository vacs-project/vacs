import {afterEach, beforeEach, describe, expect, it, vi} from "vitest";

type EventHandler = (event: {payload: unknown}) => void;

const {invoke, listen, handlers} = vi.hoisted(() => {
    const handlers = new Map<string, EventHandler>();
    return {
        handlers,
        invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
            Promise.resolve(undefined),
        ),
        listen: vi.fn<(event: string, handler: EventHandler) => Promise<() => void>>(
            (event, handler) => {
                handlers.set(event, handler);
                return Promise.resolve(() => {
                    handlers.delete(event);
                });
            },
        ),
    };
});

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {setupSignalingListeners} from "../../src/listeners/signaling-listener.ts";
import {useAuthStore} from "../../src/stores/auth-store.ts";
import {useBlinkStore} from "../../src/stores/blink-store.ts";
import {useCallListStore} from "../../src/stores/call-list-store.ts";
import {useCallStore} from "../../src/stores/call-store.ts";
import {useConnectionStore} from "../../src/stores/connection-store.ts";
import {useProfileStore} from "../../src/stores/profile-store.ts";
import {useStationsStore} from "../../src/stores/stations-store.ts";
import type {CallTarget} from "../../src/types/call.ts";
import type {SessionInfo} from "../../src/types/client.ts";
import type {CallId, ClientId, PositionId, StationId} from "../../src/types/generic.ts";
import {flushMicrotasks, makeTestCall, makeTestCallDisplay} from "../util.ts";

const CALL_ID = "call0" as CallId;
const STATION_1: CallTarget = {station: "station1" as StationId};
const STATION_2: CallTarget = {station: "station2" as StationId};

const SESSION_INFO: SessionInfo = {
    client: {
        id: "client0" as ClientId,
        positionId: "LOVV_CTR" as PositionId,
        displayName: "LOVV_N_CTR",
        frequency: "134.350",
    },
    profile: {type: "unchanged"},
    defaultCallSources: [],
    maxConfSize: 5,
};

const emit = (event: string, payload: unknown) => handlers.get(event)!({payload});

let teardown: () => void;

beforeEach(async () => {
    teardown = setupSignalingListeners();
    await flushMicrotasks();
});

afterEach(() => {
    teardown();
    vi.clearAllMocks();
    handlers.clear();
    useCallStore.getState().actions.reset();
    useCallListStore.getState().actions.clearCallList();
    useBlinkStore.getState().stopBlink();
    useConnectionStore.setState({
        connectionState: "disconnected",
        info: {displayName: "", positionId: undefined, frequency: ""},
    });
    useProfileStore.getState().reset();
    useStationsStore.getState().reset();
});

describe("signaling listeners", () => {
    it("applies the session info on connect", () => {
        emit("signaling:connected", SESSION_INFO);

        expect(useConnectionStore.getState().connectionState).toBe("connected");
        expect(useConnectionStore.getState().info.displayName).toBe("LOVV_N_CTR");
        expect(useCallStore.getState().maxConferenceSize).toBe(5);
    });

    it("builds the outgoing display from the outgoing-call event", () => {
        emit("signaling:outgoing-call", {
            callId: CALL_ID,
            source: {clientId: "client0" as ClientId},
            targets: [STATION_1, STATION_2],
            prio: false,
        });

        const display = useCallStore.getState().callDisplay;
        expect(display?.type).toBe("outgoing");
        expect(display?.call.invitedTargets).toEqual([STATION_1, STATION_2]);
        expect(useCallListStore.getState().callList.get(CALL_ID)?.name).toBe("CONF");
    });

    it("applies a call update to the displayed call", () => {
        useAuthStore.setState({cid: "client0" as ClientId});
        useCallStore.setState({callDisplay: makeTestCallDisplay("outgoing")});

        emit("signaling:call-update", {
            callId: CALL_ID,
            invitedTargets: [],
            joinedParticipants: {
                ["client0" as ClientId]: {station: "station0" as StationId},
                ["client1" as ClientId]: STATION_1,
            },
        });

        const display = useCallStore.getState().callDisplay;
        expect(display?.type).toBe("accepted");
        expect(Object.keys(display!.call.joinedParticipants)).toEqual(["client0", "client1"]);
    });

    it("keeps a still-outgoing display on a call end", () => {
        const display = makeTestCallDisplay("outgoing");
        useCallStore.setState({callDisplay: display});

        emit("signaling:call-end", CALL_ID);

        expect(useCallStore.getState().callDisplay).toBe(display);
    });

    it("clears a still-outgoing display on a forced call end", () => {
        useCallStore.setState({callDisplay: makeTestCallDisplay("outgoing")});

        emit("signaling:force-call-end", CALL_ID);

        expect(useCallStore.getState().callDisplay).toBeUndefined();
    });

    it("marks the rejected targets of the displayed call", () => {
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("outgoing", {invitedTargets: [STATION_1, STATION_2]}),
        });

        emit("signaling:call-reject", {callId: CALL_ID, targets: [STATION_2]});

        const display = useCallStore.getState().callDisplay;
        expect(display?.rejectedTargets).toEqual([STATION_2]);
        expect(display?.call.invitedTargets).toEqual([STATION_1]);
    });

    it("adds an invitation to the incoming calls", () => {
        emit("signaling:call-invitation", makeTestCall("incoming", {callId: CALL_ID}));

        expect(useCallStore.getState().incomingCalls).toHaveLength(1);
        expect(useBlinkStore.getState().blinkTimeoutId).toBeDefined();
    });

    it("resets the call state and the call list on disconnect", () => {
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted"),
            incomingCalls: [makeTestCall("incoming", {callId: "call1" as CallId})],
            conferenceState: "active",
        });
        useCallListStore
            .getState()
            .actions.addOutgoingCallListEntry({callId: CALL_ID, targets: [STATION_1]});
        useConnectionStore.setState({connectionState: "connected"});

        emit("signaling:disconnected", null);

        expect(useConnectionStore.getState().connectionState).toBe("disconnected");
        expect(useConnectionStore.getState().info).toEqual({
            displayName: "",
            positionId: undefined,
            frequency: "",
        });
        expect(useCallStore.getState().callDisplay).toBeUndefined();
        expect(useCallStore.getState().incomingCalls).toEqual([]);
        expect(useCallStore.getState().conferenceState).toBe("inactive");
        expect(useCallListStore.getState().callList.size).toBe(0);
    });

    it("stops listening after teardown", async () => {
        teardown();
        // The unlisten functions are awaited, so detaching lands a microtask later.
        await flushMicrotasks();

        expect(handlers.has("signaling:connected")).toBe(false);
    });
});
