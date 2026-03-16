import {act} from "@testing-library/preact";
import {useCallStore} from "../src/stores/call-store.ts";
import {CallId, ClientId, PositionId, StationId} from "../src/types/generic.ts";
import {Call} from "../src/types/call.ts";

export function flipBlink() {
    return act(() => {
        useCallStore.setState(s => ({blink: !s.blink}));
    });
}

export function makeTestCall(overrides: Partial<Call> = {}): Call {
    return {
        callId: "call0" as CallId,
        source: {
            clientId: "client0" as ClientId,
            positionId: "position0" as PositionId,
            stationId: "station0" as StationId,
        },
        target: {station: "station1" as StationId},
        prio: false,
        ...overrides,
    };
}
