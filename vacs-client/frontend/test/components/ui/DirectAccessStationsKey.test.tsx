import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {act, cleanup, fireEvent, render, screen} from "@testing-library/preact";
import DirectAccessStationsKey from "../../../src/components/ui/DirectAccessStationsKey.tsx";
import {ButtonColors} from "../../../src/components/ui/Button.tsx";
import {useCallStore} from "../../../src/stores/call-store.ts";
import {useAuthStore} from "../../../src/stores/auth-store.ts";
import {useStationsStore} from "../../../src/stores/stations-store.ts";
import type {CallId, ClientId, StationId} from "../../../src/types/generic.ts";
import {makeTestCall, makeTestCallDisplay} from "../../util.ts";

const A = "LOWI_TWR" as StationId;
const B = "LOWI_GND" as StationId;
const KEY = [A, B];

function renderKey() {
    render(<DirectAccessStationsKey data={{label: ["LOWI", "ALL"], stationIds: KEY}} />);
}

async function click() {
    await act(async () => {
        fireEvent.click(screen.getByRole("button"));
    });
}

afterEach(() => {
    cleanup();
    useCallStore.getState().actions.reset();
    useStationsStore.getState().reset();
    invoke.mockReset();
    invoke.mockImplementation(() => Promise.resolve(undefined));
});

describe("DirectAccessStationsKey with a conference key", () => {
    it("renders its label in the normal text color", () => {
        renderKey();

        expect(screen.getByText("LOWI")).toBeDefined();
        expect(screen.getByRole("button").className).not.toContain("text-gray-500");
    });

    it("is disabled while all of its stations are offline", () => {
        renderKey();

        expect(screen.getByRole("button").hasAttribute("disabled")).toBe(true);
    });

    it("invites the online stations in one call when pressed", async () => {
        useAuthStore.setState({cid: "1000000" as ClientId});
        useStationsStore.setState({
            stations: new Map<StationId, boolean>([
                [A, false],
                [B, true],
            ]),
        });
        renderKey();

        await click();

        expect(invoke).toHaveBeenCalledWith("signaling_invite_to_call", {
            source: {clientId: "1000000", positionId: undefined, stationId: undefined},
            targets: [{station: A}],
            prio: false,
        });
    });

    it("stays gray and does nothing on press during a call with its stations", async () => {
        useAuthStore.setState({cid: "1000000" as ClientId});
        useStationsStore.setState({
            stations: new Map<StationId, boolean>([
                [A, false],
                [B, false],
            ]),
        });
        const display = makeTestCallDisplay("accepted", {
            callId: "call0" as CallId,
            invitedTargets: [{station: B}],
        });
        useCallStore.setState({
            callDisplay: {
                ...display,
                call: {
                    ...display.call,
                    joinedParticipants: {
                        ["peer0" as ClientId]: {target: {station: A}, state: "connected"},
                    },
                },
            },
        });
        renderKey();

        expect(screen.getByRole("button")).toHaveClasses(ButtonColors.gray);
        expect(screen.getByRole("button").querySelector("div")).toBeNull();

        await click();

        const signaling = invoke.mock.calls.filter(([cmd]) => cmd.startsWith("signaling_"));
        expect(signaling).toEqual([]);
    });

    it("does not light up for an incoming call from one of its stations", () => {
        useStationsStore.setState({stations: new Map<StationId, boolean>([[A, false]])});
        useCallStore.setState({
            incomingCalls: [
                makeTestCall("incoming", {
                    source: {clientId: "client9" as ClientId, stationId: A},
                }),
            ],
        });
        renderKey();

        expect(screen.getByRole("button")).toHaveClasses(ButtonColors.gray);
        expect(screen.getByRole("button").querySelector("div")).toBeNull();
    });
});
