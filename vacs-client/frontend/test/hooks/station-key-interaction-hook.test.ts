import {describe, expect, it, afterEach, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {renderHook, act} from "@testing-library/preact";
import {
    conferenceKeyTargets,
    shouldInvite,
    useStationKeyInteraction,
} from "../../src/hooks/station-key-interaction-hook.ts";
import {useStationsStore} from "../../src/stores/stations-store.ts";
import {
    CallDisplay,
    CallDisplayType,
    ConferenceState,
    useCallStore,
} from "../../src/stores/call-store.ts";
import {useSettingsStore} from "../../src/stores/settings-store.ts";
import {useBlinkStore} from "../../src/stores/blink-store.ts";
import {useAuthStore} from "../../src/stores/auth-store.ts";
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
const CONF_A = "LOWI_TWR" as StationId;
const CONF_B = "LOWI_GND" as StationId;
const CONF_C = "LOWI_APP" as StationId;

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
            endActiveCallOnAccept: false,
            useDefaultCallSources: true,
        },
    });
});

describe("useStationKeyInteraction", () => {
    describe("basic states", () => {
        it("returns disabled when stationId is undefined", () => {
            const result = renderHook(() => useStationKeyInteraction([]));
            expectInteraction(result, {
                color: "gray",
                highlight: undefined,
                disabled: true,
                own: false,
            });
        });

        it("returns disabled when station is not online", () => {
            const result = renderHook(() => useStationKeyInteraction([FOREIGN_STATION]));
            expectInteraction(result, {
                color: "gray",
                highlight: undefined,
                disabled: true,
                own: false,
            });
        });

        it("returns enabled for an online station", () => {
            setStations([{id: FOREIGN_STATION, own: false}]);
            const result = renderHook(() => useStationKeyInteraction([FOREIGN_STATION]));
            expectInteraction(result, {disabled: false, own: false});
        });

        it("returns own for own station", () => {
            setOwnStations(OWN_STATION);
            const result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
            expectInteraction(result, {disabled: false, own: true});
        });
    });

    describe("call sources", () => {
        describe("click behavior", () => {
            it("first click on own station sets temporary source", async () => {
                setOwnStations(OWN_STATION);
                const {result} = renderHook(() => useStationKeyInteraction([OWN_STATION]));

                await act(() => result.current.handleClick());

                expect(useStationsStore.getState().defaultSource).toBeUndefined();
                expect(useStationsStore.getState().temporarySource).toBe(OWN_STATION);
            });

            it("second click promotes temporary to default source", async () => {
                setOwnStations(OWN_STATION);
                const {result} = renderHook(() => useStationKeyInteraction([OWN_STATION]));

                await act(() => result.current.handleClick());
                expect(useStationsStore.getState().defaultSource).toBeUndefined();
                expect(useStationsStore.getState().temporarySource).toBe(OWN_STATION);

                await act(() => result.current.handleClick());
                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
            });

            it("third click clears default source", async () => {
                setOwnStations(OWN_STATION);
                const {result} = renderHook(() => useStationKeyInteraction([OWN_STATION]));

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
                const {result} = renderHook(() => useStationKeyInteraction([OWN_STATION]));

                await act(() => result.current.handleClick());
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
                expect(useStationsStore.getState().defaultSource).toBe(OTHER_OWN_STATION);
            });

            it("clicking non-source own station sets it as temporary", async () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);
                useStationsStore.setState({defaultSource: OTHER_OWN_STATION});

                const {result} = renderHook(() => useStationKeyInteraction([OWN_STATION]));

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

                const {result} = renderHook(() => useStationKeyInteraction([OWN_STATION]));

                await act(() => result.current.handleClick());
                expect(useStationsStore.getState().temporarySource).toBe(OWN_STATION);
                expect(useStationsStore.getState().defaultSource).toEqual(FOREIGN_STATION);
            });
        });

        describe("colors", () => {
            it("shows honey for default source", () => {
                setOwnStations(OWN_STATION);
                useStationsStore.setState({defaultSource: OWN_STATION});

                const result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
            });

            it("shows peach for temporary source", () => {
                setOwnStations(OWN_STATION);
                useStationsStore.setState({temporarySource: OWN_STATION});

                const result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "peach", disabled: false, own: true});
            });

            it("shows gray for own station that is not a source", () => {
                setOwnStations(OWN_STATION);

                const result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
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
                let result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
            });

            it("auto-selects default source when stations arrive after position defaults", async () => {
                await act(() => {
                    useStationsStore
                        .getState()
                        .setPositionDefaultSources([OWN_STATION, OTHER_OWN_STATION]);
                });

                expect(useStationsStore.getState().defaultSource).toBeUndefined();

                await act(() => {
                    setOwnStations(OWN_STATION, OTHER_OWN_STATION);
                });

                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
                let result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
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
                let result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
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
                let result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction([FOREIGN_STATION]));
                expectInteraction(result, {color: "gray", disabled: false, own: false});
            });

            it("does not override manually set default source", async () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);
                useStationsStore.setState({defaultSource: OTHER_OWN_STATION});

                await act(() => {
                    useStationsStore.getState().setPositionDefaultSources([OWN_STATION]);
                });

                expect(useStationsStore.getState().defaultSource).toBe(OTHER_OWN_STATION);
                let result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
            });

            it("keeps existing default source when setting is toggled off", async () => {
                setOwnStations(OWN_STATION);

                await act(() => {
                    useStationsStore.getState().setPositionDefaultSources([OWN_STATION]);
                });
                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                let result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
                expectInteraction(result, {color: "gray", disabled: true, own: false});

                await act(() => {
                    useSettingsStore.getState().setCallConfig({
                        ...useSettingsStore.getState().callConfig,
                        useDefaultCallSources: false,
                    });
                });

                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
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
                let result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
                expectInteraction(result, {color: "gray", disabled: true, own: false});

                await act(() => {
                    useSettingsStore.getState().setCallConfig({
                        ...useSettingsStore.getState().callConfig,
                        useDefaultCallSources: true,
                    });
                });

                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
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
                let result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
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
                let result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
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
                let result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "gray", disabled: false, own: false});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
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
                let result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "gray", disabled: false, own: false});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
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
                let result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "gray", disabled: false, own: false});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
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
                let result = renderHook(() => useStationKeyInteraction([OWN_STATION]));
                expectInteraction(result, {color: "gray", disabled: false, own: false});
                result = renderHook(() => useStationKeyInteraction([OTHER_OWN_STATION]));
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
            const {result} = renderHook(() => useStationKeyInteraction([FOREIGN_STATION]));

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
            const {result} = renderHook(() => useStationKeyInteraction([FOREIGN_STATION]));

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
            const {result} = renderHook(() => useStationKeyInteraction([SECOND_FOREIGN_STATION]));

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
            const {result} = renderHook(() => useStationKeyInteraction([SECOND_FOREIGN_STATION]));

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
            const {result} = renderHook(() => useStationKeyInteraction([SECOND_FOREIGN_STATION]));

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
            const {result} = renderHook(() => useStationKeyInteraction([FOREIGN_STATION]));

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
            const {result} = renderHook(() => useStationKeyInteraction([FOREIGN_STATION]));

            await act(() => result.current.handleClick());

            expect(invoke).toHaveBeenCalledTimes(1);
            expect(invoke).toHaveBeenCalledWith("signaling_end_call", {callId: "call0"});
            expect(useCallStore.getState().callDisplay).toBeUndefined();
        });
    });

    describe("clicking a station that calls during another call", () => {
        function ringDuringCall(type: CallDisplayType, endActiveCallOnAccept: boolean) {
            setStations([
                {id: OWN_STATION, own: true},
                {id: FOREIGN_STATION, own: false},
                {id: SECOND_FOREIGN_STATION, own: false},
            ]);
            useSettingsStore.setState(state => ({
                callConfig: {...state.callConfig, endActiveCallOnAccept},
            }));
            useCallStore.setState({
                callDisplay: makeCallDisplay({
                    type,
                    invitedTargets: type === "outgoing" ? [SECOND_FOREIGN_STATION] : [],
                    ownInvitedTargets: type === "outgoing" ? [SECOND_FOREIGN_STATION] : [],
                    joinedStations:
                        type === "accepted" ? [OWN_STATION, SECOND_FOREIGN_STATION] : [],
                }),
                incomingCalls: [
                    makeTestCall("incoming", {
                        callId: "call1" as CallId,
                        source: {
                            clientId: "client9" as ClientId,
                            positionId: "position9" as PositionId,
                            stationId: FOREIGN_STATION,
                        },
                    }),
                ],
            });
            return renderHook(() => useStationKeyInteraction([FOREIGN_STATION]));
        }

        it("ignores the click while the setting is off", async () => {
            const {result} = ringDuringCall("accepted", false);

            await act(() => result.current.handleClick());

            expect(invoke).not.toHaveBeenCalled();
        });

        it("accepts the call while the setting is on", async () => {
            const {result} = ringDuringCall("accepted", true);

            await act(() => result.current.handleClick());

            expect(invoke).toHaveBeenCalledTimes(1);
            expect(invoke).toHaveBeenCalledWith("signaling_accept_call", {callId: "call1"});
        });

        it.each<CallDisplayType>(["outgoing", "rejected", "error"])(
            "ignores the click during a %s call even with the setting on",
            async type => {
                const {result} = ringDuringCall(type, true);

                await act(() => result.current.handleClick());

                expect(invoke).not.toHaveBeenCalled();
            },
        );
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

            const {result} = renderHook(() => useStationKeyInteraction([FOREIGN_STATION]));

            expect(result.current.color).toBe("yellow");
            expect(result.current.highlight).toBe("green");
        });
    });
});

const CONFERENCE_KEY = [CONF_A, CONF_B, CONF_C];

// Value false = online, covered by someone else; true = own; absent = offline.
const allOnline = new Map<StationId, boolean>([
    [CONF_A, false],
    [CONF_B, false],
    [CONF_C, false],
]);

describe("conferenceKeyTargets", () => {
    it("lists every online station that is not ours when idle", () => {
        expect(conferenceKeyTargets(CONFERENCE_KEY, undefined, allOnline)).toEqual([
            target(CONF_A),
            target(CONF_B),
            target(CONF_C),
        ]);
    });

    it("leaves out offline and own stations", () => {
        const stations = new Map<StationId, boolean>([
            [CONF_A, true],
            [CONF_B, false],
        ]);

        expect(conferenceKeyTargets(CONFERENCE_KEY, undefined, stations)).toEqual([target(CONF_B)]);
    });

    it("leaves out stations already joined or ringing in the call", () => {
        const display = makeCallDisplay({
            type: "accepted",
            joinedStations: [CONF_A],
            invitedTargets: [CONF_B],
        });

        expect(conferenceKeyTargets(CONFERENCE_KEY, display, allOnline)).toEqual([target(CONF_C)]);
    });
});

describe("shouldInvite", () => {
    const targets = [target(CONF_B), target(CONF_C)];

    it("invites when idle", () => {
        expect(shouldInvite(targets, undefined, "inactive")).toBe(true);
    });

    it("does nothing without targets", () => {
        expect(shouldInvite([], undefined, "inactive")).toBe(false);
        expect(
            shouldInvite(
                [],
                makeCallDisplay({type: "accepted", joinedStations: [CONF_A]}),
                "modify",
            ),
        ).toBe(false);
    });

    it.each<ConferenceState>(["active", "inactive"])(
        "does nothing in a call outside modify mode (%s)",
        conferenceState => {
            for (const isConferenceLeader of [true, false, undefined]) {
                const display = makeCallDisplay({
                    type: "accepted",
                    joinedStations: [CONF_A],
                    isConferenceLeader,
                });
                expect(shouldInvite(targets, display, conferenceState)).toBe(false);
            }
        },
    );

    it("invites in modify mode as the leader or in a 1:1 call", () => {
        for (const isConferenceLeader of [true, undefined]) {
            const display = makeCallDisplay({
                type: "accepted",
                joinedStations: [CONF_A],
                isConferenceLeader,
            });
            expect(shouldInvite(targets, display, "modify")).toBe(true);
        }
    });

    it("does nothing in modify mode as a non-leader of a conference", () => {
        const display = makeCallDisplay({
            type: "accepted",
            joinedStations: [CONF_A],
            isConferenceLeader: false,
        });

        expect(shouldInvite(targets, display, "modify")).toBe(false);
    });
});

describe("useStationKeyInteraction with several stations", () => {
    function setup(
        display: CallDisplay | undefined,
        conferenceState: ConferenceState = "inactive",
        stations: Map<StationId, boolean> = allOnline,
    ) {
        useAuthStore.setState({cid: "1000000" as ClientId});
        useStationsStore.setState({stations: new Map(stations)});
        useCallStore.setState({callDisplay: display, conferenceState});
        return renderHook(() => useStationKeyInteraction(CONFERENCE_KEY));
    }

    const press = (hook: ReturnType<typeof setup>) =>
        act(async () => {
            await hook.result.current.handleClick();
        });

    it("starts a call to every invitable station when idle", async () => {
        const hook = setup(undefined);

        await press(hook);

        expect(invoke).toHaveBeenCalledWith("signaling_invite_to_call", {
            source: {clientId: "1000000", positionId: undefined, stationId: undefined},
            targets: [target(CONF_A), target(CONF_B), target(CONF_C)],
            prio: false,
        });
    });

    it("invites the remaining stations in modify mode", async () => {
        const hook = setup(
            makeCallDisplay({type: "accepted", joinedStations: [CONF_A], isConferenceLeader: true}),
            "modify",
        );

        await press(hook);

        expect(invoke).toHaveBeenCalledWith(
            "signaling_invite_to_call",
            expect.objectContaining({targets: [target(CONF_B), target(CONF_C)]}),
        );
    });

    it("neither invites nor ends the call outside modify mode", async () => {
        const display = makeCallDisplay({
            type: "accepted",
            joinedStations: [CONF_A],
            isConferenceLeader: true,
        });
        const hook = setup(display, "active");

        await press(hook);

        expect(invoke).not.toHaveBeenCalled();
        expect(useCallStore.getState().callDisplay).toBe(display);
    });

    it("shows its default color whatever the call state", () => {
        const idle = setup(undefined).result.current;
        const inCall = setup(
            makeCallDisplay({type: "accepted", joinedStations: [CONF_A], invitedTargets: [CONF_B]}),
        ).result.current;

        expect(inCall.color).toBe(idle.color);
        expect(inCall.highlight).toBe(idle.highlight);
    });

    it("is disabled while none of its stations is online", () => {
        expect(setup(undefined, "inactive", new Map()).result.current.disabled).toBe(true);
    });

    it("is disabled while its only online stations are ours", () => {
        const own = new Map<StationId, boolean>([
            [CONF_A, true],
            [CONF_B, true],
        ]);

        expect(setup(undefined, "inactive", own).result.current.disabled).toBe(true);
    });
});
