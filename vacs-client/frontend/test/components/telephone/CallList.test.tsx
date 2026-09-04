import {afterEach, beforeAll, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {cleanup, render, screen} from "@testing-library/preact";
import CallList from "../../../src/components/telephone/CallList.tsx";
import {useCallListStore} from "../../../src/stores/call-list-store.ts";
import {useConnectionStore} from "../../../src/stores/connection-store.ts";
import type {CallId, ClientId} from "../../../src/types/generic.ts";

const CALL_ID = "call0" as CallId;
const CLIENT_A = "1000001" as ClientId;
const CLIENT_B = "1000002" as ClientId;

beforeAll(() => {
    // jsdom has no ResizeObserver; List measures itself with one to size its rows.
    vi.stubGlobal(
        "ResizeObserver",
        class {
            observe() {}
            unobserve() {}
            disconnect() {}
        },
    );
});

const ignoreButton = () => screen.getByRole<HTMLButtonElement>("button", {name: /ignore/i});
const callButton = () => screen.getByRole<HTMLButtonElement>("button", {name: /^call$/i});

afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    useCallListStore.getState().actions.clearCallList();
    useConnectionStore.setState({connectionState: "disconnected"});
});

describe("CallList", () => {
    it("disables Call and Ignore and shows both client ids for a two-target entry", () => {
        useConnectionStore.setState({connectionState: "connected"});
        useCallListStore.getState().actions.addOutgoingCallListEntry({
            callId: CALL_ID,
            targets: [{client: CLIENT_A}, {client: CLIENT_B}],
        });

        render(<CallList />);

        expect(ignoreButton().disabled).toBe(true);
        expect(callButton().disabled).toBe(true);
        expect(screen.getByText(`${CLIENT_A}, ${CLIENT_B}`)).toBeDefined();
    });

    it("enables Call and Ignore for a single-target entry while connected", () => {
        useConnectionStore.setState({connectionState: "connected"});
        useCallListStore.getState().actions.addOutgoingCallListEntry({
            callId: CALL_ID,
            targets: [{client: CLIENT_A}],
        });

        render(<CallList />);

        expect(ignoreButton().disabled).toBe(false);
        expect(callButton().disabled).toBe(false);
    });
});
