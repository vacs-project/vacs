import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {cleanup, render, screen} from "@testing-library/preact";
import CallQueue from "../../src/components/CallQueue.tsx";
import {CallDisplay, ConnectionState, useCallStore} from "../../src/stores/call-store.ts";
import type {CallId, ClientId, StationId} from "../../src/types/generic.ts";
import {makeTestCall, makeTestCallDisplay} from "../util.ts";

const SELF = "client0" as ClientId;
const PEER = "client1" as ClientId;

function acceptedWithStates(self: ConnectionState, peer: ConnectionState): CallDisplay {
    const display = makeTestCallDisplay("accepted", {invitedTargets: []});
    display.call.joinedParticipants[SELF].state = self;
    display.call.joinedParticipants[PEER].state = peer;
    return display;
}

afterEach(() => {
    cleanup();
    useCallStore.getState().actions.reset();
});

describe("CallQueue", () => {
    it("shows the disconnected icon in preference to the degraded one", () => {
        useCallStore.setState({callDisplay: acceptedWithStates("degraded", "disconnected")});

        render(<CallQueue />);

        expect(screen.getByAltText("Disconnected")).not.toBeNull();
        expect(screen.queryByAltText("No incoming audio")).toBeNull();
    });

    it("shows the degraded icon while no peer is disconnected", () => {
        useCallStore.setState({callDisplay: acceptedWithStates("connected", "degraded")});

        render(<CallQueue />);

        expect(screen.getByAltText("No incoming audio")).not.toBeNull();
        expect(screen.queryByAltText("Disconnected")).toBeNull();
    });

    it("shows no connection icon while every peer is connected", () => {
        useCallStore.setState({callDisplay: acceptedWithStates("connected", "connected")});

        render(<CallQueue />);

        expect(screen.queryByAltText("Disconnected")).toBeNull();
        expect(screen.queryByAltText("No incoming audio")).toBeNull();
    });

    it("labels a call with two other parties CONF", () => {
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted", {
                invitedTargets: [{station: "station2" as StationId}],
            }),
        });

        render(<CallQueue />);

        expect(screen.getByText("CONF")).not.toBeNull();
    });

    it("labels a 1:1 call with the other party", () => {
        useCallStore.setState({callDisplay: makeTestCallDisplay("accepted", {invitedTargets: []})});

        render(<CallQueue />);

        expect(screen.queryByText("CONF")).toBeNull();
        expect(screen.getByText("station0")).not.toBeNull();
    });

    it("labels an incoming conference invitation CONF", () => {
        useCallStore.setState({
            incomingCalls: [
                makeTestCall("incoming", {
                    callId: "call1" as CallId,
                    joinedParticipants: {
                        ["client8" as ClientId]: {station: "station8" as StationId},
                    },
                }),
            ],
        });

        render(<CallQueue />);

        expect(screen.getByText("CONF")).not.toBeNull();
    });

    it("labels a 1:1 incoming call with the caller", () => {
        useCallStore.setState({
            incomingCalls: [makeTestCall("incoming", {callId: "call1" as CallId})],
        });

        render(<CallQueue />);

        expect(screen.queryByText("CONF")).toBeNull();
        expect(screen.getByText("station0")).not.toBeNull();
    });
});
