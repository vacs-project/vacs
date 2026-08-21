import {act} from "@testing-library/preact";
import {CallId, ClientId, PositionId, StationId} from "../src/types/generic.ts";
import {useBlinkStore} from "../src/stores/blink-store.ts";
import {CallDisplay, CallDisplayType} from "../src/stores/call-store.ts";
import {Call, CallTarget} from "../src/types/call.ts";

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
        joinedParticipants: {},
        prio: false,
        ...overrides,
    };
}

export function makeTestCallDisplay(
    type: CallDisplayType,
    overrides: Partial<Call> = {},
    prioTargets: CallTarget[] = [],
): CallDisplay {
    return {
        type,
        call: {
            ...makeTestCall(type, overrides),
            joinedParticipants:
                type === "accepted"
                    ? {
                          ["client0" as ClientId]: {
                              target: {station: "station0" as StationId},
                              state: "connecting",
                          },
                          ["client1" as ClientId]: {
                              target: {station: "station1" as StationId},
                              state: "connecting",
                          },
                      }
                    : {},
            isConferenceLeader: undefined,
        },
        prioTargets,
        erroredTargets: [],
        rejectedTargets: [],
    };
}
