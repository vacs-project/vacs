import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {renderHook} from "@testing-library/preact";
import {useCallState} from "../../src/hooks/call-state-hook.ts";
import {useAuthStore} from "../../src/stores/auth-store.ts";
import {useBlinkStore} from "../../src/stores/blink-store.ts";
import {useCallStore} from "../../src/stores/call-store.ts";
import {useSettingsStore} from "../../src/stores/settings-store.ts";
import type {Call, CallTarget} from "../../src/types/call.ts";
import type {DirectAccessPage} from "../../src/types/profile.ts";
import type {CallId, ClientId, StationId} from "../../src/types/generic.ts";
import {makeTestCall, makeTestCallDisplay} from "../util.ts";

const OWN_CLIENT = "client0" as ClientId;
const PAGE_STATION = "LOWI_APP" as StationId;
const NESTED_STATION = "LOWS_APP" as StationId;
const FOREIGN_STATION = "LOWW_APP" as StationId;

const PAGE_TARGET: CallTarget = {station: PAGE_STATION};

// A page whose key list reaches PAGE_STATION directly and NESTED_STATION via a subpage.
const PAGE: DirectAccessPage = {
    rows: 2,
    keys: [
        {label: ["LOWI"], stationId: PAGE_STATION},
        {
            label: ["MORE"],
            page: {rows: 1, keys: [{label: ["LOWS"], stationId: NESTED_STATION}]},
        },
    ],
};

const incomingCall = (overrides: Partial<Call> = {}): Call =>
    makeTestCall("incoming", {
        callId: "call1" as CallId,
        source: {clientId: "client9" as ClientId, stationId: FOREIGN_STATION},
        target: {station: FOREIGN_STATION},
        ...overrides,
    });

const state = () => renderHook(() => useCallState(PAGE)).result.current;

afterEach(() => {
    useCallStore.getState().actions.reset();
    useBlinkStore.setState({blink: false});
    useSettingsStore.setState({
        callConfig: {
            highlightIncomingCallTarget: true,
            enablePriorityCalls: true,
            enableCallStartSound: true,
            enableCallEndSound: true,
            enableParticipantJoinedSound: true,
            enableParticipantLeftSound: true,
            useDefaultCallSources: true,
            forceRelay: false,
        },
    });
});

describe("useCallState", () => {
    describe("isCalling", () => {
        it("matches an incoming call whose source is on the page", () => {
            useCallStore.setState({
                incomingCalls: [
                    incomingCall({
                        source: {clientId: "client9" as ClientId, stationId: PAGE_STATION},
                    }),
                ],
            });

            expect(state().isCalling).toBe(true);
        });

        it("matches an incoming call that a page station has already joined", () => {
            useCallStore.setState({
                incomingCalls: [
                    incomingCall({
                        joinedParticipants: {["client8" as ClientId]: {station: NESTED_STATION}},
                    }),
                ],
            });

            expect(state().isCalling).toBe(true);
        });

        it("matches an incoming call from a stationless source once a page station joined", () => {
            useCallStore.setState({
                incomingCalls: [
                    incomingCall({
                        source: {clientId: "client9" as ClientId},
                        joinedParticipants: {["client8" as ClientId]: {station: PAGE_STATION}},
                    }),
                ],
            });

            expect(state().isCalling).toBe(true);
        });

        it("ignores an incoming call that no page station is part of", () => {
            useCallStore.setState({incomingCalls: [incomingCall()]});

            expect(state().isCalling).toBe(false);
        });
    });

    describe("inCall", () => {
        function acceptedWithJoined(joined: Record<string, CallTarget>) {
            const display = makeTestCallDisplay("accepted");
            useCallStore.setState({
                callDisplay: {
                    ...display,
                    call: {
                        ...display.call,
                        joinedParticipants: Object.fromEntries(
                            Object.entries(joined).map(([clientId, target]) => [
                                clientId,
                                {target, state: "connected" as const},
                            ]),
                        ),
                    },
                },
            });
        }

        it("matches a page station held by another participant", () => {
            useAuthStore.setState({cid: OWN_CLIENT});
            acceptedWithJoined({[OWN_CLIENT]: {station: FOREIGN_STATION}, client1: PAGE_TARGET});

            expect(state().inCall).toBe(true);
        });

        it("ignores a page station that is this client's own", () => {
            useAuthStore.setState({cid: OWN_CLIENT});
            acceptedWithJoined({[OWN_CLIENT]: PAGE_TARGET});

            expect(state().inCall).toBe(false);
        });

        it("is false while the call is still outgoing", () => {
            useAuthStore.setState({cid: OWN_CLIENT});
            const display = makeTestCallDisplay("outgoing", {invitedTargets: [PAGE_TARGET]});
            useCallStore.setState({callDisplay: display});

            const current = state();
            expect(current.inCall).toBe(false);
            expect(current.beingCalled).toBe(true);
        });
    });

    describe("per-target annotations", () => {
        it("reports a rejected page station", () => {
            const display = makeTestCallDisplay("rejected", {invitedTargets: []});
            useCallStore.setState({
                callDisplay: {...display, rejectedTargets: [PAGE_TARGET]},
            });

            expect(state().isRejected).toBe(true);
            expect(state().isError).toBe(false);
        });

        it("ignores a rejection of a station that is not on the page", () => {
            const display = makeTestCallDisplay("rejected", {invitedTargets: []});
            useCallStore.setState({
                callDisplay: {...display, rejectedTargets: [{station: FOREIGN_STATION}]},
            });

            expect(state().isRejected).toBe(false);
        });

        it("reports an errored page station reached through a subpage", () => {
            const display = makeTestCallDisplay("error", {invitedTargets: []});
            useCallStore.setState({
                callDisplay: {
                    ...display,
                    erroredTargets: [{target: {station: NESTED_STATION}, reason: "callFailure"}],
                },
            });

            expect(state().isError).toBe(true);
        });
    });

    describe("prio", () => {
        function acceptedWithPagePeer(prioTargets: CallTarget[]) {
            useAuthStore.setState({cid: OWN_CLIENT});
            const display = makeTestCallDisplay("accepted", {invitedTargets: []}, prioTargets);
            useCallStore.setState({
                callDisplay: {
                    ...display,
                    call: {
                        ...display.call,
                        joinedParticipants: {
                            [OWN_CLIENT]: {
                                target: {station: FOREIGN_STATION},
                                state: "connected",
                            },
                            ["client1" as ClientId]: {target: PAGE_TARGET, state: "connected"},
                        },
                    },
                },
            });
        }

        it("marks a joined prio target", () => {
            acceptedWithPagePeer([PAGE_TARGET]);

            const current = state();
            expect(current.color).toBe("yellow");
            expect(current.highlight).toBe("green");
        });

        it("leaves a joined target without prio plain", () => {
            acceptedWithPagePeer([]);

            const current = state();
            expect(current.color).toBe("green");
            expect(current.highlight).toBeUndefined();
        });

        it("takes prio from an incoming call", () => {
            useBlinkStore.setState({blink: true});
            useCallStore.setState({
                incomingCalls: [
                    incomingCall({
                        source: {clientId: "client9" as ClientId, stationId: PAGE_STATION},
                        prio: true,
                    }),
                ],
            });

            const current = state();
            expect(current.color).toBe("yellow");
            expect(current.highlight).toBe("green");
        });

        it("ignores prio when priority calls are disabled", () => {
            useSettingsStore.setState({
                callConfig: {
                    ...useSettingsStore.getState().callConfig,
                    enablePriorityCalls: false,
                },
            });
            acceptedWithPagePeer([PAGE_TARGET]);

            expect(state().color).toBe("green");
        });
    });

    describe("isTarget", () => {
        it("marks the page station an incoming call is addressed to", () => {
            useCallStore.setState({incomingCalls: [incomingCall({target: PAGE_TARGET})]});

            expect(state().isTarget).toBe(true);
        });

        it("marks the page station an accepted call is addressed to", () => {
            useCallStore.setState({
                callDisplay: makeTestCallDisplay("accepted", {
                    target: PAGE_TARGET,
                    invitedTargets: [],
                }),
            });

            expect(state().isTarget).toBe(true);
        });

        it("stays unmarked when the highlight setting is off", () => {
            useSettingsStore.setState({
                callConfig: {
                    ...useSettingsStore.getState().callConfig,
                    highlightIncomingCallTarget: false,
                },
            });
            useCallStore.setState({incomingCalls: [incomingCall({target: PAGE_TARGET})]});

            expect(state().isTarget).toBe(false);
        });
    });
});
