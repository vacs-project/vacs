import {describe, expect, it} from "vitest";
import {CallTarget, incomingOtherPartyCount} from "../../src/types/call.ts";
import {ClientId, StationId} from "../../src/types/generic.ts";

const CALLER = "client9" as ClientId;
const OTHER = "client1" as ClientId;

const STATION_1: CallTarget = {station: "station1" as StationId};
const STATION_9: CallTarget = {station: "station9" as StationId};

function call(invitedTargets: CallTarget[], joinedParticipants: Record<ClientId, CallTarget>) {
    return {source: {clientId: CALLER}, invitedTargets, joinedParticipants};
}

describe("incomingOtherPartyCount", () => {
    it("counts a plain 1:1 ring as one other party", () => {
        expect(incomingOtherPartyCount(call([], {}))).toBe(1);
    });

    it("counts the still-ringing caller of a fresh multi-target call", () => {
        expect(incomingOtherPartyCount(call([STATION_1], {}))).toBe(2);
    });

    it("does not double-count a caller that already joined", () => {
        expect(incomingOtherPartyCount(call([], {[CALLER]: STATION_9, [OTHER]: STATION_1}))).toBe(
            2,
        );
    });

    it("counts joined participants and ringing targets together", () => {
        expect(
            incomingOtherPartyCount(call([STATION_1], {[CALLER]: STATION_9, [OTHER]: STATION_1})),
        ).toBe(3);
    });
});
