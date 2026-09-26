import {describe, expect, it, afterEach, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {renderHook, act} from "@testing-library/preact";
import {useStationKeyInteraction} from "../../src/hooks/station-key-interaction-hook.ts";
import {useStationsStore} from "../../src/stores/stations-store.ts";
import {CallDisplay, CallDisplayType, useCallStore} from "../../src/stores/call-store.ts";
import {useSettingsStore} from "../../src/stores/settings-store.ts";
import {useBlinkStore} from "../../src/stores/blink-store.ts";
import type {CallId, ClientId, PositionId, StationId} from "../../src/types/generic.ts";
import type {ButtonColor, ButtonHighlightColor} from "../../src/components/ui/Button.tsx";
import type {StationInfo} from "../../src/types/station.ts";
import type {CallParticipantsWithConnectionState, CallTarget} from "../../src/types/call.ts";
import {makeTestCall, makeTestCallDisplay} from "../util.ts";

const OWN_STATION = "LOVV_N1" as StationId;
const OTHER_OWN_STATION = "LOVV_N2" as StationId;
const FOREIGN_STATION = "LOWI_APP" as StationId;
const SECOND_FOREIGN_STATION = "LOWW_APP" as StationId;
const THIRD_FOREIGN_STATION = "LOWS_APP" as StationId;

type InteractionResult = ReturnType<typeof useStationKeyInteraction>;
type ExpectedInteraction = {
    color?: ButtonColor;
    highlight?: ButtonHighlightColor;
    disabled: boolean;
    own: boolean;
};

function expectInteraction(
    {result}: {result: {current: InteractionResult}},
    expected: ExpectedInteraction,
) {
    if (expected.color === undefined) {
        expected.color = "gray";
    }
    expect(result.current.color).toBe(expected.color);
    expect(result.current.highlight).toBe(expected.highlight);
    expect(result.current.disabled).toBe(expected.disabled);
    expect(result.current.own).toBe(expected.own);
}

function setStations(stations: StationInfo[]) {
    useStationsStore.getState().setStations(stations);
}

function setOwnStations(...ids: StationId[]) {
    setStations(ids.map(id => ({id, own: true})));
}

type CallDisplayOptions = {
    type: CallDisplayType;
    invitedTargets?: StationId[];
    ownInvitedTargets?: StationId[];
    joinedStations?: StationId[];
    isConferenceLeader?: boolean;
};

function target(station: StationId): CallTarget {
    return {station};
}

function makeCallDisplay(options: CallDisplayOptions): CallDisplay {
    const invitedTargets = (options.invitedTargets ?? []).map(target);
    const base = makeTestCallDisplay("outgoing", {invitedTargets});

    const joinedParticipants: CallParticipantsWithConnectionState = {};
    (options.joinedStations ?? []).forEach((station, index) => {
        joinedParticipants[`client${index}` as ClientId] = {
            target: target(station),
            state: "connected",
        };
    });

    return {
        ...base,
        type: options.type,
        call: {
            ...base.call,
            joinedParticipants,
            ownInvitedTargets: (options.ownInvitedTargets ?? []).map(target),
            isConferenceLeader: options.isConferenceLeader,
        },
    };
}

afterEach(() => {
    vi.clearAllMocks();
    useStationsStore.getState().reset();
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
            forceRelay: false,
            useDefaultCallSources: true,
        },
    });
});

describe("useStationKeyInteraction", () => {
    describe("basic states", () => {
        it("returns disabled when stationId is undefined", () => {
            const result = renderHook(() => useStationKeyInteraction(undefined));
            expectInteraction(result, {
                color: "gray",
                highlight: undefined,
                disabled: true,
                own: false,
            });
        });

        it("returns disabled when station is not online", () => {
            const result = renderHook(() => useStationKeyInteraction(FOREIGN_STATION));
            expectInteraction(result, {
                color: "gray",
                highlight: undefined,
                disabled: true,
                own: false,
            });
        });

        it("returns enabled for an online station", () => {
            setStations([{id: FOREIGN_STATION, own: false}]);
            const result = renderHook(() => useStationKeyInteraction(FOREIGN_STATION));
            expectInteraction(result, {disabled: false, own: false});
        });

        it("returns own for own station", () => {
            setOwnStations(OWN_STATION);
            const result = renderHook(() => useStationKeyInteraction(OWN_STATION));
            expectInteraction(result, {disabled: false, own: true});
        });
    });

    describe("call sources", () => {
        describe("click behavior", () => {
            it("first click on own station sets temporary source", async () => {
                setOwnStations(OWN_STATION);
                const {result} = renderHook(() => useStationKeyInteraction(OWN_STATION));

                await act(() => result.current.handleClick());

                expect(useStationsStore.getState().defaultSource).toBeUndefined();
                expect(useStationsStore.getState().temporarySource).toBe(OWN_STATION);
            });

            it("second click promotes temporary to default source", async () => {
                setOwnStations(OWN_STATION);
                const {result} = renderHook(() => useStationKeyInteraction(OWN_STATION));

                await act(() => result.current.handleClick());
                expect(useStationsStore.getState().defaultSource).toBeUndefined();
                expect(useStationsStore.getState().temporarySource).toBe(OWN_STATION);

                await act(() => result.current.handleClick());
                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
            });

            it("third click clears default source", async () => {
                setOwnStations(OWN_STATION);
                const {result} = renderHook(() => useStationKeyInteraction(OWN_STATION));

                await act(() => result.current.handleClick());
                expect(useStationsStore.getState().defaultSource).toBeUndefined();
                expect(useStationsStore.getState().temporarySource).toBe(OWN_STATION);

                await act(() => result.current.handleClick());
                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                expect(useStationsStore.getState().temporarySource).toBeUndefined();

                await act(() => result.current.handleClick());
                expect(useStationsStore.getState().defaultSource).toBeUndefined();
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
            });

            it("clicking temporary source when different default exists clears temporary", async () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);
                useStationsStore.setState({
                    defaultSource: OTHER_OWN_STATION,
                    temporarySource: OWN_STATION,
                });
                const {result} = renderHook(() => useStationKeyInteraction(OWN_STATION));

                await act(() => result.current.handleClick());
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
                expect(useStationsStore.getState().defaultSource).toBe(OTHER_OWN_STATION);
            });

            it("clicking non-source own station sets it as temporary", async () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);
                useStationsStore.setState({defaultSource: OTHER_OWN_STATION});

                const {result} = renderHook(() => useStationKeyInteraction(OWN_STATION));

                await act(() => result.current.handleClick());
                expect(useStationsStore.getState().temporarySource).toBe(OWN_STATION);
                expect(useStationsStore.getState().defaultSource).toBe(OTHER_OWN_STATION);
            });

            it("clicking non-source own station switches temporary source", async () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION, FOREIGN_STATION);
                useStationsStore.setState({
                    temporarySource: OTHER_OWN_STATION,
                    defaultSource: FOREIGN_STATION,
                });

                const {result} = renderHook(() => useStationKeyInteraction(OWN_STATION));

                await act(() => result.current.handleClick());
                expect(useStationsStore.getState().temporarySource).toBe(OWN_STATION);
                expect(useStationsStore.getState().defaultSource).toEqual(FOREIGN_STATION);
            });
        });

        describe("colors", () => {
            it("shows honey for default source", () => {
                setOwnStations(OWN_STATION);
                useStationsStore.setState({defaultSource: OWN_STATION});

                const result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
            });

            it("shows peach for temporary source", () => {
                setOwnStations(OWN_STATION);
                useStationsStore.setState({temporarySource: OWN_STATION});

                const result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "peach", disabled: false, own: true});
            });

            it("shows gray for own station that is not a source", () => {
                setOwnStations(OWN_STATION);

                const result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
            });
        });

        describe("default call sources from position", () => {
            it("auto-selects default source from position defaults when enabled", async () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);

                await act(() => {
                    useStationsStore
                        .getState()
                        .setPositionDefaultSources([OWN_STATION, OTHER_OWN_STATION]);
                });

                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
            });

            it("does not auto-select when useDefaultCallSources is disabled", async () => {
                useSettingsStore.setState({
                    callConfig: {
                        ...useSettingsStore.getState().callConfig,
                        useDefaultCallSources: false,
                    },
                });
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);

                await act(() => {
                    useStationsStore
                        .getState()
                        .setPositionDefaultSources([OWN_STATION, OTHER_OWN_STATION]);
                });

                expect(useStationsStore.getState().defaultSource).toBeUndefined();
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
            });

            it("picks first own station from position default sources", async () => {
                setStations([
                    {id: FOREIGN_STATION, own: false},
                    {id: OWN_STATION, own: true},
                    {id: OTHER_OWN_STATION, own: true},
                ]);

                await act(() => {
                    useStationsStore
                        .getState()
                        .setPositionDefaultSources([
                            FOREIGN_STATION,
                            OWN_STATION,
                            OTHER_OWN_STATION,
                        ]);
                });

                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(FOREIGN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: false});
            });

            it("does not override manually set default source", async () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);
                useStationsStore.setState({defaultSource: OTHER_OWN_STATION});

                await act(() => {
                    useStationsStore.getState().setPositionDefaultSources([OWN_STATION]);
                });

                expect(useStationsStore.getState().defaultSource).toBe(OTHER_OWN_STATION);
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
            });

            it("keeps existing default source when setting is toggled off", async () => {
                setOwnStations(OWN_STATION);

                await act(() => {
                    useStationsStore.getState().setPositionDefaultSources([OWN_STATION]);
                });
                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: true, own: false});

                await act(() => {
                    useSettingsStore.getState().setCallConfig({
                        ...useSettingsStore.getState().callConfig,
                        useDefaultCallSources: false,
                    });
                });

                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: true, own: false});
            });

            it("auto-selects default source when setting is toggled on with no existing default", async () => {
                setOwnStations(OWN_STATION);

                await act(() => {
                    useSettingsStore.getState().setCallConfig({
                        ...useSettingsStore.getState().callConfig,
                        useDefaultCallSources: false,
                    });
                });

                await act(() => {
                    useStationsStore.getState().setPositionDefaultSources([OWN_STATION]);
                });

                expect(useStationsStore.getState().defaultSource).toBeUndefined();
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: true, own: false});

                await act(() => {
                    useSettingsStore.getState().setCallConfig({
                        ...useSettingsStore.getState().callConfig,
                        useDefaultCallSources: true,
                    });
                });

                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: true, own: false});
            });

            it("clears temporary source if it matches newly auto-selected default", async () => {
                setOwnStations(OWN_STATION);
                useStationsStore.setState({temporarySource: OWN_STATION});

                await act(() => {
                    useStationsStore.getState().setPositionDefaultSources([OWN_STATION]);
                });

                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: true, own: false});
            });

            it("keeps temporary source if it differs from auto-selected default", async () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);
                useStationsStore.setState({temporarySource: OTHER_OWN_STATION});

                await act(() => {
                    useStationsStore.getState().setPositionDefaultSources([OWN_STATION]);
                });

                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                expect(useStationsStore.getState().temporarySource).toBe(OTHER_OWN_STATION);
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "peach", disabled: false, own: true});
            });
        });

        describe("coverage changes", () => {
            it("resets default source when station is no longer covered", async () => {
                setOwnStations(OWN_STATION);
                useStationsStore.setState({
                    defaultSource: OWN_STATION,
                    positionDefaultSources: [],
                });

                await act(() => {
                    setStations([{id: OWN_STATION, own: false}]);
                });

                expect(useStationsStore.getState().defaultSource).toBeUndefined();
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: false});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: true, own: false});
            });

            it("resets temporary source when station is no longer covered", async () => {
                setOwnStations(OWN_STATION);
                useStationsStore.setState({temporarySource: OWN_STATION});

                await act(() => {
                    setStations([{id: OWN_STATION, own: false}]);
                });

                expect(useStationsStore.getState().defaultSource).toBeUndefined();
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: false});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: true, own: false});
            });

            it("chooses next available default source when original station is no longer covered", async () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);
                useStationsStore.setState({
                    defaultSource: OWN_STATION,
                    positionDefaultSources: [OWN_STATION, OTHER_OWN_STATION],
                });

                await act(() => {
                    setStations([
                        {id: OWN_STATION, own: false},
                        {id: OTHER_OWN_STATION, own: true},
                    ]);
                });

                expect(useStationsStore.getState().defaultSource).toBe(OTHER_OWN_STATION);
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: false});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
            });

            it("promotes temporary source to default when original default station is no longer covered", async () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);
                useStationsStore.setState({
                    defaultSource: OWN_STATION,
                    temporarySource: OTHER_OWN_STATION,
                    positionDefaultSources: [OWN_STATION, OTHER_OWN_STATION],
                });

                await act(() => {
                    setStations([
                        {id: OWN_STATION, own: false},
                        {id: OTHER_OWN_STATION, own: true},
                    ]);
                });

                expect(useStationsStore.getState().defaultSource).toBe(OTHER_OWN_STATION);
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: false});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
            });
        });
    });

    describe("clicking a station that is part of the current call", () => {
        it("cancels one of two ringing targets it invited itself", async () => {
            setStations([
                {id: OWN_STATION, own: true},
                {id: FOREIGN_STATION, own: false},
                {id: SECOND_FOREIGN_STATION, own: false},
            ]);
            useCallStore.setState({
                callDisplay: makeCallDisplay({
                    type: "outgoing",
                    invitedTargets: [FOREIGN_STATION, SECOND_FOREIGN_STATION],
                    ownInvitedTargets: [FOREIGN_STATION, SECOND_FOREIGN_STATION],
                    isConferenceLeader: true,
                }),
                conferenceState: "active",
            });
            const {result} = renderHook(() => useStationKeyInteraction(FOREIGN_STATION));

            await act(() => result.current.handleClick());

            expect(invoke).toHaveBeenCalledTimes(1);
            expect(invoke).toHaveBeenCalledWith("signaling_drop_target", {
                callId: "call0",
                target: {station: FOREIGN_STATION},
            });
            const callDisplay = useCallStore.getState().callDisplay;
            expect(callDisplay?.call.invitedTargets).toEqual([{station: SECOND_FOREIGN_STATION}]);
        });

        it("keeps the invitation when the server refuses the drop", async () => {
            setStations([
                {id: FOREIGN_STATION, own: false},
                {id: SECOND_FOREIGN_STATION, own: false},
            ]);
            const display = makeCallDisplay({
                type: "outgoing",
                invitedTargets: [FOREIGN_STATION, SECOND_FOREIGN_STATION],
                ownInvitedTargets: [FOREIGN_STATION, SECOND_FOREIGN_STATION],
                isConferenceLeader: true,
            });
            useCallStore.setState({callDisplay: display});
            invoke.mockRejectedValueOnce({
                title: "Call",
                detail: "Not allowed",
                isNonCritical: true,
            });
            const {result} = renderHook(() => useStationKeyInteraction(FOREIGN_STATION));

            await act(() => result.current.handleClick());

            expect(useCallStore.getState().callDisplay).toBe(display);
            expect(useCallStore.getState().callDisplay?.call.invitedTargets).toEqual([
                {station: FOREIGN_STATION},
                {station: SECOND_FOREIGN_STATION},
            ]);
        });

        it("does nothing for a pending invitation of another participant", async () => {
            setStations([
                {id: OWN_STATION, own: true},
                {id: FOREIGN_STATION, own: false},
                {id: SECOND_FOREIGN_STATION, own: false},
            ]);
            const display = makeCallDisplay({
                type: "accepted",
                invitedTargets: [SECOND_FOREIGN_STATION],
                ownInvitedTargets: [],
                joinedStations: [OWN_STATION, FOREIGN_STATION],
                isConferenceLeader: false,
            });
            useCallStore.setState({callDisplay: display});
            const {result} = renderHook(() => useStationKeyInteraction(SECOND_FOREIGN_STATION));

            await act(() => result.current.handleClick());

            expect(invoke).not.toHaveBeenCalled();
            expect(useCallStore.getState().callDisplay).toBe(display);
            // The key still renders as ringing, so the click really hit the
            // display-only branch instead of falling through to startCall.
            expect(result.current.highlight).toBe("green");
        });

        it("drops a joined participant as conference leader without changing local state", async () => {
            setStations([
                {id: OWN_STATION, own: true},
                {id: FOREIGN_STATION, own: false},
                {id: SECOND_FOREIGN_STATION, own: false},
                {id: THIRD_FOREIGN_STATION, own: false},
            ]);
            const display = makeCallDisplay({
                type: "accepted",
                joinedStations: [
                    OWN_STATION,
                    FOREIGN_STATION,
                    SECOND_FOREIGN_STATION,
                    THIRD_FOREIGN_STATION,
                ],
                isConferenceLeader: true,
            });
            useCallStore.setState({callDisplay: display, conferenceState: "active"});
            const {result} = renderHook(() => useStationKeyInteraction(SECOND_FOREIGN_STATION));

            await act(() => result.current.handleClick());

            expect(invoke).toHaveBeenCalledTimes(1);
            expect(invoke).toHaveBeenCalledWith("signaling_drop_target", {
                callId: "call0",
                target: {station: SECOND_FOREIGN_STATION},
            });
            // The participant is only removed once the server confirms the drop.
            expect(useCallStore.getState().callDisplay).toBe(display);
            expect(Object.keys(display.call.joinedParticipants)).toHaveLength(4);
        });

        it("ends the call when a non-leader clicks a joined participant", async () => {
            setStations([
                {id: OWN_STATION, own: true},
                {id: FOREIGN_STATION, own: false},
                {id: SECOND_FOREIGN_STATION, own: false},
                {id: THIRD_FOREIGN_STATION, own: false},
            ]);
            useCallStore.setState({
                callDisplay: makeCallDisplay({
                    type: "accepted",
                    joinedStations: [
                        OWN_STATION,
                        FOREIGN_STATION,
                        SECOND_FOREIGN_STATION,
                        THIRD_FOREIGN_STATION,
                    ],
                    isConferenceLeader: false,
                }),
                conferenceState: "active",
            });
            const {result} = renderHook(() => useStationKeyInteraction(SECOND_FOREIGN_STATION));

            await act(() => result.current.handleClick());

            expect(invoke).toHaveBeenCalledTimes(1);
            expect(invoke).toHaveBeenCalledWith("signaling_end_call", {callId: "call0"});
            expect(useCallStore.getState().callDisplay).toBeUndefined();
        });

        it("ends the call when the leader clicks the only other participant", async () => {
            setStations([
                {id: OWN_STATION, own: true},
                {id: FOREIGN_STATION, own: false},
            ]);
            useCallStore.setState({
                callDisplay: makeCallDisplay({
                    type: "accepted",
                    joinedStations: [OWN_STATION, FOREIGN_STATION],
                    isConferenceLeader: true,
                }),
            });
            const {result} = renderHook(() => useStationKeyInteraction(FOREIGN_STATION));

            await act(() => result.current.handleClick());

            expect(invoke).toHaveBeenCalledTimes(1);
            expect(invoke).toHaveBeenCalledWith("signaling_end_call", {callId: "call0"});
            expect(useCallStore.getState().callDisplay).toBeUndefined();
        });

        it("ends the call when cancelling the only ringing target of a 1:1 call", async () => {
            setStations([{id: FOREIGN_STATION, own: false}]);
            useCallStore.setState({
                callDisplay: makeCallDisplay({
                    type: "outgoing",
                    invitedTargets: [FOREIGN_STATION],
                    ownInvitedTargets: [FOREIGN_STATION],
                }),
            });
            const {result} = renderHook(() => useStationKeyInteraction(FOREIGN_STATION));

            await act(() => result.current.handleClick());

            expect(invoke).toHaveBeenCalledTimes(1);
            expect(invoke).toHaveBeenCalledWith("signaling_end_call", {callId: "call0"});
            expect(useCallStore.getState().callDisplay).toBeUndefined();
        });
    });

    describe("prio", () => {
        it("shows priority while an incoming prio call rings during another call", async () => {
            setStations([
                {id: OWN_STATION, own: true},
                {id: FOREIGN_STATION, own: false},
                {id: SECOND_FOREIGN_STATION, own: false},
            ]);
            useCallStore.setState({
                callDisplay: makeCallDisplay({
                    type: "accepted",
                    joinedStations: [OWN_STATION, SECOND_FOREIGN_STATION],
                }),
                incomingCalls: [
                    makeTestCall("incoming", {
                        callId: "call1" as CallId,
                        source: {
                            clientId: "client9" as ClientId,
                            positionId: "position9" as PositionId,
                            stationId: FOREIGN_STATION,
                        },
                        prio: true,
                    }),
                ],
            });
            await act(() => {
                useBlinkStore.setState({blink: true});
            });

            const {result} = renderHook(() => useStationKeyInteraction(FOREIGN_STATION));

            expect(result.current.color).toBe("yellow");
            expect(result.current.highlight).toBe("green");
        });
    });
});
