import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {cleanup, render, screen} from "@testing-library/preact";
import InfoGrid from "../../src/components/InfoGrid.tsx";
import {useCallStore} from "../../src/stores/call-store.ts";
import type {StationId} from "../../src/types/generic.ts";
import {makeTestCallDisplay} from "../util.ts";

const STATION_1 = {station: "station1" as StationId};
const STATION_2 = {station: "station2" as StationId};

afterEach(() => {
    cleanup();
    useCallStore.getState().actions.reset();
});

describe("InfoGrid", () => {
    it("shows the whole-call error reason", () => {
        const display = makeTestCallDisplay("error", {invitedTargets: []});
        useCallStore.setState({
            callDisplay: {
                ...display,
                errorReason: "callFailure",
                erroredTargets: [{target: STATION_1, reason: "autoHangup"}],
            },
        });

        render(<InfoGrid />);

        expect(screen.getByTitle("callFailure")).not.toBeNull();
    });

    it("falls back to the first errored target's reason", () => {
        const display = makeTestCallDisplay("accepted", {invitedTargets: []});
        useCallStore.setState({
            callDisplay: {
                ...display,
                erroredTargets: [
                    {target: STATION_1, reason: "peerConnectionFailed"},
                    {target: STATION_2, reason: "autoHangup"},
                ],
            },
        });

        render(<InfoGrid />);

        expect(screen.getByTitle("peerConnectionFailed")).not.toBeNull();
        expect(screen.queryByTitle("autoHangup")).toBeNull();
    });

    it("shows no reason while the call carries no error", () => {
        useCallStore.setState({callDisplay: makeTestCallDisplay("accepted", {invitedTargets: []})});

        render(<InfoGrid />);

        expect(screen.queryByTitle("callFailure")).toBeNull();
    });
});
