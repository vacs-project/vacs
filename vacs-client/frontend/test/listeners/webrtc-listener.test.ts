import {afterEach, describe, expect, it, vi} from "vitest";

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

import {setupWebrtcListeners} from "../../src/listeners/webrtc-listener.ts";
import {useCallStore} from "../../src/stores/call-store.ts";
import type {CallTarget} from "../../src/types/call.ts";
import type {CallId, ClientId, StationId} from "../../src/types/generic.ts";
import {flushMicrotasks, makeTestCallDisplay} from "../util.ts";

const CALL_ID = "call0" as CallId;
const PEER_A = "client1" as ClientId;
const PEER_B = "client2" as ClientId;
const STATION_1: CallTarget = {station: "station1" as StationId};
const STATION_2: CallTarget = {station: "station2" as StationId};
const STATION_3: CallTarget = {station: "station3" as StationId};

const emit = (event: string, payload: unknown) => handlers.get(event)!({payload});

function setJoinedCallDisplay() {
    const display = makeTestCallDisplay("accepted", {
        callId: CALL_ID,
        invitedTargets: [STATION_3],
    });
    useCallStore.setState({
        callDisplay: {
            ...display,
            call: {
                ...display.call,
                joinedParticipants: {
                    [PEER_A]: {target: STATION_1, state: "connecting"},
                    [PEER_B]: {target: STATION_2, state: "connecting"},
                },
            },
        },
    });
}

let teardown: () => void;

afterEach(() => {
    if (teardown !== undefined) teardown();
    vi.clearAllMocks();
    handlers.clear();
    useCallStore.getState().actions.reset();
});

describe("webrtc listeners", () => {
    it("degrades only the named peer's connection state", async () => {
        setJoinedCallDisplay();
        teardown = setupWebrtcListeners();
        await flushMicrotasks();

        emit("webrtc:call-degraded", {callId: CALL_ID, peerId: PEER_A});

        const participants = useCallStore.getState().callDisplay!.call.joinedParticipants;
        expect(participants[PEER_A].state).toBe("degraded");
        expect(participants[PEER_B].state).toBe("connecting");
    });

    it("reconnects only the named peer's connection state", async () => {
        setJoinedCallDisplay();
        teardown = setupWebrtcListeners();
        await flushMicrotasks();

        emit("webrtc:call-degraded", {callId: CALL_ID, peerId: PEER_A});
        emit("webrtc:call-connected", {callId: CALL_ID, peerId: PEER_A});

        const participants = useCallStore.getState().callDisplay!.call.joinedParticipants;
        expect(participants[PEER_A].state).toBe("connected");
        expect(participants[PEER_B].state).toBe("connecting");
    });

    it("moves a named error target from invited to errored", async () => {
        setJoinedCallDisplay();
        teardown = setupWebrtcListeners();
        await flushMicrotasks();

        emit("webrtc:call-error", {
            callId: CALL_ID,
            origin: {type: "targets", value: [STATION_3]},
            reason: "callFailure",
            callEnded: false,
        });

        const display = useCallStore.getState().callDisplay;
        expect(display?.call.invitedTargets).toEqual([]);
        expect(display?.erroredTargets).toEqual([{target: STATION_3, reason: "callFailure"}]);
    });

    it("stops listening after teardown", async () => {
        setJoinedCallDisplay();
        teardown = setupWebrtcListeners();
        await flushMicrotasks();

        teardown();
        await flushMicrotasks();

        expect(handlers.has("webrtc:call-connected")).toBe(false);
    });
});
