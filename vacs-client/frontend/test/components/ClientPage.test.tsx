import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {cleanup, render, screen} from "@testing-library/preact";
import ClientPage from "../../src/components/ClientPage.tsx";
import {ButtonColors} from "../../src/components/ui/Button.tsx";
import {useClientsStore} from "../../src/stores/clients-store.ts";
import {useFilterStore} from "../../src/stores/filter-store.ts";
import {useBlinkStore} from "../../src/stores/blink-store.ts";
import {useCallStore} from "../../src/stores/call-store.ts";
import type {ClientInfo, ClientPageConfig} from "../../src/types/client.ts";
import type {CallId, ClientId} from "../../src/types/generic.ts";
import {flipBlink, makeTestCall, makeTestCallDisplay} from "../util.ts";

const CLIENT_1 = "1000001" as ClientId;
const CLIENT_2 = "1000002" as ClientId;
const OUTSIDER = "9000000" as ClientId;

const GROUP_CLIENTS: ClientInfo[] = [
    {id: CLIENT_1, positionId: undefined, displayName: "EDDF_APP", frequency: "119.900"},
    {id: CLIENT_2, positionId: undefined, displayName: "EDDF_TWR", frequency: "121.500"},
];

const CONFIG: ClientPageConfig = {
    frequencies: "ShowAll",
    grouping: "Icao",
};

function groupButton() {
    return screen.getByTitle("EDDF").closest("button");
}

afterEach(() => {
    cleanup();
    useCallStore.getState().actions.reset();
    useClientsStore.setState({clients: []});
    useFilterStore.setState({filter: ""});
    useBlinkStore.setState({blink: false});
});

describe("ClientPage group key", () => {
    it("blinks green for a rejected client in the group", async () => {
        useClientsStore.setState({clients: GROUP_CLIENTS});
        const display = makeTestCallDisplay("rejected", {
            source: {clientId: OUTSIDER},
            invitedTargets: [{client: CLIENT_1}],
        });
        // Mirror what rejectTargets() does: drop the target from invitedTargets and
        // move it into rejectedTargets.
        display.call.invitedTargets = [];
        display.rejectedTargets = [{client: CLIENT_1}];
        useCallStore.setState({callDisplay: display});
        await flipBlink();

        render(<ClientPage config={CONFIG} />);

        expect(groupButton()).toHaveClasses(ButtonColors.green);
    });

    it("blinks red for an errored client in the group", async () => {
        useClientsStore.setState({clients: GROUP_CLIENTS});
        const display = makeTestCallDisplay("error", {
            source: {clientId: OUTSIDER},
            invitedTargets: [{client: CLIENT_1}],
        });
        // Mirror what errorTargets() does: drop the target from invitedTargets and
        // move it into erroredTargets.
        display.call.invitedTargets = [];
        display.erroredTargets = [{target: {client: CLIENT_1}, reason: "callFailure"}];
        useCallStore.setState({callDisplay: display});
        await flipBlink();

        render(<ClientPage config={CONFIG} />);

        expect(groupButton()).toHaveClasses(ButtonColors.red);
    });

    it("lights for an incoming call whose joined participant is in the group", async () => {
        useClientsStore.setState({clients: GROUP_CLIENTS});
        useCallStore.setState({
            incomingCalls: [
                makeTestCall("incoming", {
                    callId: "call1" as CallId,
                    source: {clientId: OUTSIDER},
                    joinedParticipants: {[CLIENT_1]: {client: CLIENT_1}},
                }),
            ],
        });
        await flipBlink();

        render(<ClientPage config={CONFIG} />);

        expect(groupButton()).toHaveClasses(ButtonColors.green);
    });

    it("stays gray for a call that involves nobody in the group", async () => {
        useClientsStore.setState({clients: GROUP_CLIENTS});
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted", {
                source: {clientId: OUTSIDER},
                invitedTargets: [{client: "9000001" as ClientId}],
            }),
        });
        await flipBlink();

        render(<ClientPage config={CONFIG} />);

        expect(groupButton()).toHaveClasses(ButtonColors.gray);
    });
});
