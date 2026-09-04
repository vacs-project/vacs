import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {act, cleanup, fireEvent, render, screen} from "@testing-library/preact";
import DirectAccessClientKey from "../../../src/components/ui/DirectAccessClientKey.tsx";
import {
    ButtonColor,
    ButtonColors,
    ButtonHighlightColor,
    ButtonHighlightColors,
} from "../../../src/components/ui/Button.tsx";
import {CallDisplay, CallDisplayType, useCallStore} from "../../../src/stores/call-store.ts";
import {useAuthStore} from "../../../src/stores/auth-store.ts";
import type {CallParticipantsWithConnectionState} from "../../../src/types/call.ts";
import type {ClientInfo} from "../../../src/types/client.ts";
import type {CallId, ClientId} from "../../../src/types/generic.ts";
import {makeTestCallDisplay} from "../../util.ts";

const CALL_ID = "call0" as CallId;
const OWN = "1000000" as ClientId;
const PEER = "1000001" as ClientId;
const SECOND_PEER = "1000002" as ClientId;
const THIRD_PEER = "1000003" as ClientId;

const peerClient: ClientInfo = {
    id: PEER,
    positionId: undefined,
    displayName: "LOWI_APP",
    frequency: "119.400",
};

type DisplayOptions = {
    type: CallDisplayType;
    invitedTargets?: ClientId[];
    ownInvitedTargets?: ClientId[];
    joinedClients?: ClientId[];
    isConferenceLeader?: boolean;
    rejectedTargets?: ClientId[];
    erroredTargets?: ClientId[];
};

function makeClientCallDisplay(options: DisplayOptions): CallDisplay {
    const invitedTargets = (options.invitedTargets ?? []).map(client => ({client}));
    const base = makeTestCallDisplay("outgoing", {callId: CALL_ID, invitedTargets});

    const joinedParticipants: CallParticipantsWithConnectionState = {};
    for (const client of options.joinedClients ?? []) {
        joinedParticipants[client] = {target: {client}, state: "connected"};
    }

    return {
        ...base,
        type: options.type,
        call: {
            ...base.call,
            source: {clientId: OWN},
            target: {client: PEER},
            joinedParticipants,
            ownInvitedTargets: (options.ownInvitedTargets ?? []).map(client => ({client})),
            isConferenceLeader: options.isConferenceLeader,
        },
        rejectedTargets: (options.rejectedTargets ?? []).map(client => ({client})),
        erroredTargets: (options.erroredTargets ?? []).map(client => ({
            target: {client},
            reason: "callFailure",
        })),
    };
}

function renderKey() {
    render(<DirectAccessClientKey client={peerClient} config={undefined} />);
}

async function click() {
    await act(async () => {
        fireEvent.click(screen.getByRole("button"));
    });
}

function expectColorAndHighlight(color: ButtonColor, highlight: ButtonHighlightColor) {
    const button = screen.getByRole("button");
    expect(button).toHaveClasses(ButtonColors[color]);
    expect(button.querySelector("div")).toHaveClasses(ButtonHighlightColors[highlight]);
}

afterEach(() => {
    cleanup();
    useCallStore.getState().actions.reset();
    invoke.mockReset();
    invoke.mockImplementation(() => Promise.resolve(undefined));
});

describe("DirectAccessClientKey", () => {
    it("starts a call for a client that is not part of any call", async () => {
        useAuthStore.setState({cid: OWN});
        renderKey();

        await click();

        expect(invoke).toHaveBeenCalledWith("signaling_invite_to_call", {
            source: {clientId: OWN, positionId: undefined, stationId: undefined},
            targets: [{client: PEER}],
            prio: false,
        });
    });

    it("cancels one of two ringing targets it invited itself", async () => {
        useCallStore.setState({
            callDisplay: makeClientCallDisplay({
                type: "outgoing",
                invitedTargets: [PEER, SECOND_PEER],
                ownInvitedTargets: [PEER, SECOND_PEER],
                isConferenceLeader: true,
            }),
            conferenceState: "active",
        });
        renderKey();

        await click();

        expect(invoke).toHaveBeenCalledWith("signaling_drop_target", {
            callId: CALL_ID,
            target: {client: PEER},
        });
        expect(useCallStore.getState().callDisplay?.call.invitedTargets).toEqual([
            {client: SECOND_PEER},
        ]);
    });

    it("keeps the invitation when the server refuses the drop", async () => {
        const display = makeClientCallDisplay({
            type: "outgoing",
            invitedTargets: [PEER, SECOND_PEER],
            ownInvitedTargets: [PEER, SECOND_PEER],
            isConferenceLeader: true,
        });
        useCallStore.setState({callDisplay: display});
        invoke.mockImplementation(cmd =>
            cmd === "signaling_drop_target"
                ? Promise.reject({title: "Call", detail: "Not allowed", isNonCritical: true})
                : Promise.resolve(undefined),
        );
        renderKey();

        await click();

        expect(useCallStore.getState().callDisplay).toBe(display);
        expect(useCallStore.getState().callDisplay?.call.invitedTargets).toEqual([
            {client: PEER},
            {client: SECOND_PEER},
        ]);
    });

    it("does nothing for a pending invitation of another participant", async () => {
        const display = makeClientCallDisplay({
            type: "accepted",
            invitedTargets: [PEER],
            ownInvitedTargets: [],
            joinedClients: [OWN, SECOND_PEER],
            isConferenceLeader: false,
        });
        useCallStore.setState({callDisplay: display});
        renderKey();

        await click();

        expect(invoke).not.toHaveBeenCalledWith("signaling_drop_target", expect.anything());
        expect(invoke).not.toHaveBeenCalledWith("signaling_end_call", expect.anything());
        expect(useCallStore.getState().callDisplay).toBe(display);
        // The key still renders as ringing, so the click hit the display-only branch.
        expectColorAndHighlight("gray", "green");
    });

    it("drops a joined participant as conference leader without changing local state", async () => {
        const display = makeClientCallDisplay({
            type: "accepted",
            joinedClients: [OWN, PEER, SECOND_PEER, THIRD_PEER],
            isConferenceLeader: true,
        });
        useCallStore.setState({callDisplay: display, conferenceState: "active"});
        renderKey();

        await click();

        expect(invoke).toHaveBeenCalledWith("signaling_drop_target", {
            callId: CALL_ID,
            target: {client: PEER},
        });
        // The participant is only removed once the server confirms the drop.
        expect(useCallStore.getState().callDisplay).toBe(display);
    });

    it("ends the call when a non-leader clicks a joined participant", async () => {
        useCallStore.setState({
            callDisplay: makeClientCallDisplay({
                type: "accepted",
                joinedClients: [OWN, PEER, SECOND_PEER, THIRD_PEER],
                isConferenceLeader: false,
            }),
            conferenceState: "active",
        });
        renderKey();

        await click();

        expect(invoke).toHaveBeenCalledWith("signaling_end_call", {callId: CALL_ID});
        expect(useCallStore.getState().callDisplay).toBeUndefined();
        expect(useCallStore.getState().conferenceState).toBe("inactive");
    });

    it("ends the call when the leader clicks the only other participant", async () => {
        useCallStore.setState({
            callDisplay: makeClientCallDisplay({
                type: "accepted",
                joinedClients: [OWN, PEER],
                isConferenceLeader: true,
            }),
        });
        renderKey();

        await click();

        expect(invoke).toHaveBeenCalledWith("signaling_end_call", {callId: CALL_ID});
        expect(useCallStore.getState().callDisplay).toBeUndefined();
    });

    it("ends the call when cancelling the only ringing target of a 1:1 call", async () => {
        useCallStore.setState({
            callDisplay: makeClientCallDisplay({
                type: "outgoing",
                invitedTargets: [PEER],
                ownInvitedTargets: [PEER],
            }),
        });
        renderKey();

        await click();

        expect(invoke).toHaveBeenCalledWith("signaling_end_call", {callId: CALL_ID});
        expect(useCallStore.getState().callDisplay).toBeUndefined();
    });

    it("dismisses the rejection of its own target", async () => {
        useCallStore.setState({
            callDisplay: makeClientCallDisplay({type: "rejected", rejectedTargets: [PEER]}),
        });
        renderKey();

        await click();

        expect(invoke).not.toHaveBeenCalledWith("signaling_end_call", expect.anything());
        expect(useCallStore.getState().callDisplay).toBeUndefined();
    });

    it("dismisses its own error annotation and keeps the live call", async () => {
        useCallStore.setState({
            callDisplay: makeClientCallDisplay({
                type: "accepted",
                joinedClients: [OWN, SECOND_PEER],
                erroredTargets: [PEER],
            }),
        });
        renderKey();

        await click();

        const display = useCallStore.getState().callDisplay;
        expect(display?.type).toBe("accepted");
        expect(display?.erroredTargets).toEqual([]);
    });
});
