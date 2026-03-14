import {describe, expect, it} from "vitest";
import {
    getCallStateColors,
    CallStateColorParams,
    CallStateColors,
} from "../../src/utils/call-state-colors.ts";

const defaults: CallStateColorParams = {
    inCall: false,
    isCalling: false,
    beingCalled: false,
    isRejected: false,
    isError: false,
    outgoingPrio: false,
    incomingPrio: false,
    blink: false,
};

function colors(overrides: Partial<CallStateColorParams>): CallStateColors {
    return getCallStateColors({...defaults, ...overrides});
}

describe("getCallStateColors", () => {
    describe("idle / default states", () => {
        it("returns gray with no highlight when idle", () => {
            expect(colors({})).toEqual({color: "gray", highlight: undefined});
        });

        it("returns sage when isTarget", () => {
            expect(colors({isTarget: true})).toEqual({color: "sage", highlight: undefined});
        });

        it("returns peach when temporarySource", () => {
            expect(colors({temporarySource: true})).toEqual({color: "peach", highlight: undefined});
        });

        it("returns honey when defaultSource", () => {
            expect(colors({defaultSource: true})).toEqual({color: "honey", highlight: undefined});
        });
    });

    describe("in call", () => {
        it("returns green with no highlight", () => {
            expect(colors({inCall: true})).toEqual({color: "green", highlight: undefined});
        });

        it("returns yellow with green highlight for priority calls", () => {
            expect(colors({inCall: true, outgoingPrio: true})).toEqual({
                color: "yellow",
                highlight: "green",
            });
        });
    });

    describe("incoming call (isCalling)", () => {
        it("returns green when blink is on", () => {
            expect(colors({isCalling: true, blink: true})).toEqual({
                color: "green",
                highlight: undefined,
            });
        });

        it("returns gray when blink is off", () => {
            expect(colors({isCalling: true, blink: false})).toEqual({
                color: "gray",
                highlight: undefined,
            });
        });

        it("returns yellow with green highlight when incoming prio and blink on", () => {
            expect(colors({isCalling: true, incomingPrio: true, blink: true})).toEqual({
                color: "yellow",
                highlight: "green",
            });
        });

        it("returns gray with gray highlight when incoming prio and blink off", () => {
            expect(colors({isCalling: true, incomingPrio: true, blink: false})).toEqual({
                color: "gray",
                highlight: "gray",
            });
        });
    });

    describe("outgoing call (beingCalled)", () => {
        it("returns gray with green highlight (non-prio)", () => {
            expect(colors({beingCalled: true})).toEqual({color: "gray", highlight: "green"});
        });

        it("returns yellow with green highlight when outgoing prio and blink on", () => {
            expect(colors({beingCalled: true, outgoingPrio: true, blink: true})).toEqual({
                color: "yellow",
                highlight: "green",
            });
        });

        it("returns gray with green highlight when outgoing prio and blink off", () => {
            expect(colors({beingCalled: true, outgoingPrio: true, blink: false})).toEqual({
                color: "gray",
                highlight: "green",
            });
        });
    });

    describe("rejected call", () => {
        it("returns green with green highlight when blink on", () => {
            expect(colors({isRejected: true, blink: true})).toEqual({
                color: "green",
                highlight: "green",
            });
        });

        it("returns gray with green highlight when blink off", () => {
            expect(colors({isRejected: true, blink: false})).toEqual({
                color: "gray",
                highlight: "green",
            });
        });
    });

    describe("error call", () => {
        it("returns red when blink on", () => {
            expect(colors({isError: true, blink: true})).toEqual({
                color: "red",
                highlight: undefined,
            });
        });

        it("returns gray when blink off", () => {
            expect(colors({isError: true, blink: false})).toEqual({
                color: "gray",
                highlight: undefined,
            });
        });
    });
});
