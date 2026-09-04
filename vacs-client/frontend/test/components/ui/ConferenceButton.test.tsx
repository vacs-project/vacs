import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {act, cleanup, fireEvent, render, screen} from "@testing-library/preact";
import ConferenceButton from "../../../src/components/ui/ConferenceButton.tsx";
import {ButtonColor, ButtonColors} from "../../../src/components/ui/Button.tsx";
import {useAuthStore} from "../../../src/stores/auth-store.ts";
import {useBlinkStore} from "../../../src/stores/blink-store.ts";
import {CallDisplay, useCallStore} from "../../../src/stores/call-store.ts";
import type {CallTarget} from "../../../src/types/call.ts";
import type {ClientId, StationId} from "../../../src/types/generic.ts";
import {makeTestCallDisplay} from "../../util.ts";

const SELF = "client0" as ClientId;
const PEER = "client1" as ClientId;
const STATION_2: CallTarget = {station: "station2" as StationId};

// An established call: a peer other than this client reports a connected leg.
function established(
    overrides: {invitedTargets?: CallTarget[]; isConferenceLeader?: boolean} = {},
): CallDisplay {
    const display = makeTestCallDisplay("accepted", {
        invitedTargets: overrides.invitedTargets ?? [],
    });
    display.call.joinedParticipants[PEER].state = "connected";
    display.call.isConferenceLeader = overrides.isConferenceLeader;
    return display;
}

const button = () => screen.getByRole<HTMLButtonElement>("button", {name: "CONF"});

async function click() {
    await act(() => {
        fireEvent.click(button());
    });
}

function expectColor(color: ButtonColor) {
    expect(button()).toHaveClasses(ButtonColors[color]);
}

afterEach(() => {
    cleanup();
    useCallStore.getState().actions.reset();
    useBlinkStore.getState().stopBlink();
    useBlinkStore.setState({blink: false});
});

describe("ConferenceButton", () => {
    it("is disabled while no call is established", () => {
        render(<ConferenceButton />);

        expect(button().disabled).toBe(true);
        expectColor("cyan");
    });

    it("is disabled while every leg is still connecting", () => {
        useAuthStore.setState({cid: SELF});
        useCallStore.setState({callDisplay: makeTestCallDisplay("accepted", {invitedTargets: []})});

        render(<ConferenceButton />);

        expect(button().disabled).toBe(true);
    });

    it("enters modify mode and returns to inactive for a 1:1 call", async () => {
        useAuthStore.setState({cid: SELF});
        useCallStore.setState({callDisplay: established()});
        render(<ConferenceButton />);

        await click();
        expect(useCallStore.getState().conferenceState).toBe("modify");

        await click();
        expect(useCallStore.getState().conferenceState).toBe("inactive");
    });

    it("returns to active when the call is already a conference", async () => {
        useAuthStore.setState({cid: SELF});
        useCallStore.setState({
            callDisplay: established({invitedTargets: [STATION_2]}),
            conferenceState: "active",
        });
        render(<ConferenceButton />);

        await click();
        expect(useCallStore.getState().conferenceState).toBe("modify");

        await click();
        expect(useCallStore.getState().conferenceState).toBe("active");
    });

    it("blinks blue while in modify mode", async () => {
        useAuthStore.setState({cid: SELF});
        useCallStore.setState({callDisplay: established(), conferenceState: "modify"});
        useBlinkStore.setState({blink: true});
        render(<ConferenceButton />);

        expectColor("blue");

        await act(() => {
            useBlinkStore.setState({blink: false});
        });
        expectColor("cyan");
    });

    it("stays blue for an inactive conference of three parties", () => {
        useAuthStore.setState({cid: SELF});
        useCallStore.setState({
            callDisplay: established({invitedTargets: [STATION_2]}),
            conferenceState: "inactive",
        });

        render(<ConferenceButton />);

        expectColor("blue");
    });

    it("locks out a participant that is not the conference leader", () => {
        useAuthStore.setState({cid: SELF});
        useCallStore.setState({
            callDisplay: established({invitedTargets: [STATION_2], isConferenceLeader: false}),
            conferenceState: "active",
        });
        render(<ConferenceButton />);

        // The lock is the disabled attribute; jsdom would still deliver a synthetic
        // click, so the store-level guard is covered in call-store-invite instead.
        expect(button().disabled).toBe(true);
    });

    it("keeps the conference open for the leader", async () => {
        useAuthStore.setState({cid: SELF});
        useCallStore.setState({
            callDisplay: established({invitedTargets: [STATION_2], isConferenceLeader: true}),
            conferenceState: "active",
        });
        render(<ConferenceButton />);

        expect(button().disabled).toBe(false);

        await click();

        expect(useCallStore.getState().conferenceState).toBe("modify");
    });
});
