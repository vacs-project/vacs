import {act} from "@testing-library/preact";
import {CallId, ClientId, PositionId, StationId} from "../src/types/generic.ts";
import {Call} from "../src/types/call.ts";
import {useBlinkStore} from "../src/stores/blink-store.ts";
import {CallDisplayType} from "../src/stores/call-store.ts";

export async function flipBlink() {
    await act(() => {
        useBlinkStore.setState(s => ({blink: !s.blink}));
    });
}

export function makeTestCall(
    type: CallDisplayType | "incoming",
    overrides: Partial<Call> = {},
): Call {
    return {
        callId: "call0" as CallId,
        source: {
            clientId: "client0" as ClientId,
            positionId: "position0" as PositionId,
            stationId: "station0" as StationId,
        },
        target: {station: "station1" as StationId},
        invitedTargets: type === "incoming" ? [] : [{station: "station1" as StationId}],
        joinedParticipants:
            type === "accepted"
                ? new Map([
                      ["client0" as ClientId, {station: "station0" as StationId}],
                      ["client1" as ClientId, {station: "station1" as StationId}],
                  ])
                : new Map(),
        prio: false,
        ...overrides,
    };
}
