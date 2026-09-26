import {describe, expect, it} from "vitest";
import {shouldStopBlinking} from "../../src/stores/blink-store.ts";
import {CallDisplay} from "../../src/stores/call-store.ts";
import {CallTarget} from "../../src/types/call.ts";
import {StationId} from "../../src/types/generic.ts";
import {makeTestCallDisplay} from "../util.ts";

const STATION_1: CallTarget = {station: "station1" as StationId};
const STATION_2: CallTarget = {station: "station2" as StationId};

const stops = (callDisplay: CallDisplay | undefined) =>
    shouldStopBlinking(0, callDisplay, false, false, "inactive");

describe("shouldStopBlinking", () => {
    it("stops without a call display", () => {
        expect(stops(undefined)).toBe(true);
    });

    it("stops for a plain outgoing call", () => {
        expect(stops(makeTestCallDisplay("outgoing"))).toBe(true);
    });

    it("keeps blinking while a prio target rings", () => {
        expect(stops(makeTestCallDisplay("outgoing", {}, [STATION_1]))).toBe(false);
    });

    it("keeps blinking while a prio conference invite rings", () => {
        const display = makeTestCallDisplay("accepted", {invitedTargets: [STATION_2]}, [STATION_2]);
        expect(stops(display)).toBe(false);
    });

    it("stops once the prio target has joined", () => {
        const display = makeTestCallDisplay("accepted", {invitedTargets: []}, [STATION_1]);
        expect(stops(display)).toBe(true);
    });

    it("keeps blinking while an accepted call carries annotations", () => {
        const display = makeTestCallDisplay("accepted", {invitedTargets: []});
        expect(stops({...display, rejectedTargets: [STATION_2]})).toBe(false);
        expect(stops({...display, erroredTargets: [{target: STATION_2, reason: "x"}]})).toBe(false);
    });

    it("never stops on a terminal display", () => {
        expect(stops(makeTestCallDisplay("rejected"))).toBe(false);
        expect(stops(makeTestCallDisplay("error"))).toBe(false);
    });

    it("never stops while incoming calls ring or the conference is being modified", () => {
        expect(shouldStopBlinking(1, undefined, false, false, "inactive")).toBe(false);
        expect(shouldStopBlinking(0, undefined, false, false, "modify")).toBe(false);
    });
});
