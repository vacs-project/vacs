import {describe, expect, it, afterEach} from "vitest";
import {renderHook, act} from "@testing-library/preact";
import {useStationKeyInteraction} from "../../src/hooks/station-key-interaction-hook.ts";
import {useStationsStore} from "../../src/stores/stations-store.ts";
import {useCallStore} from "../../src/stores/call-store.ts";
import {useSettingsStore} from "../../src/stores/settings-store.ts";
import type {StationId} from "../../src/types/generic.ts";
import type {ButtonColor, ButtonHighlightColor} from "../../src/components/ui/Button.tsx";
import type {StationInfo} from "../../src/types/station.ts";

const OWN_STATION = "LOVV_N1" as StationId;
const OTHER_OWN_STATION = "LOVV_N2" as StationId;
const FOREIGN_STATION = "LOWI_APP" as StationId;

type InteractionResult = ReturnType<typeof useStationKeyInteraction>;
type ExpectedInteraction = {
    color?: ButtonColor;
    highlight?: ButtonHighlightColor;
    disabled?: boolean;
    own?: boolean;
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
    if (expected.disabled !== undefined) expect(result.current.disabled).toBe(expected.disabled);
    if (expected.own !== undefined) expect(result.current.own).toBe(expected.own);
}

function setStations(stations: StationInfo[]) {
    useStationsStore.getState().setStations(stations);
}

function setOwnStations(...ids: StationId[]) {
    setStations(ids.map(id => ({id, own: true})));
}

afterEach(() => {
    useStationsStore.getState().reset();
    useCallStore.getState().actions.reset();
    useSettingsStore.setState({
        callConfig: {
            highlightIncomingCallTarget: true,
            enablePriorityCalls: true,
            enableCallStartSound: true,
            enableCallEndSound: true,
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
            it("auto-selects default source from position defaults when enabled", () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);

                act(() => {
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

            it("does not auto-select when useDefaultCallSources is disabled", () => {
                useSettingsStore.setState({
                    callConfig: {
                        ...useSettingsStore.getState().callConfig,
                        useDefaultCallSources: false,
                    },
                });
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);

                act(() => {
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

            it("picks first own station from position default sources", () => {
                setStations([
                    {id: FOREIGN_STATION, own: false},
                    {id: OWN_STATION, own: true},
                    {id: OTHER_OWN_STATION, own: true},
                ]);

                act(() => {
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

            it("does not override manually set default source", () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);
                useStationsStore.setState({defaultSource: OTHER_OWN_STATION});

                act(() => {
                    useStationsStore.getState().setPositionDefaultSources([OWN_STATION]);
                });

                expect(useStationsStore.getState().defaultSource).toBe(OTHER_OWN_STATION);
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
            });

            it("keeps existing default source when setting is toggled off", () => {
                setOwnStations(OWN_STATION);

                act(() => {
                    useStationsStore.getState().setPositionDefaultSources([OWN_STATION]);
                });
                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: true, own: false});

                act(() => {
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

            it("auto-selects default source when setting is toggled on with no existing default", () => {
                setOwnStations(OWN_STATION);

                act(() => {
                    useSettingsStore.getState().setCallConfig({
                        ...useSettingsStore.getState().callConfig,
                        useDefaultCallSources: false,
                    });
                });

                act(() => {
                    useStationsStore.getState().setPositionDefaultSources([OWN_STATION]);
                });

                expect(useStationsStore.getState().defaultSource).toBeUndefined();
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: true, own: false});

                act(() => {
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

            it("clears temporary source if it matches newly auto-selected default", () => {
                setOwnStations(OWN_STATION);
                useStationsStore.setState({temporarySource: OWN_STATION});

                act(() => {
                    useStationsStore.getState().setPositionDefaultSources([OWN_STATION]);
                });

                expect(useStationsStore.getState().defaultSource).toBe(OWN_STATION);
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "honey", disabled: false, own: true});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: true, own: false});
            });

            it("keeps temporary source if it differs from auto-selected default", () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);
                useStationsStore.setState({temporarySource: OTHER_OWN_STATION});

                act(() => {
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
            it("resets default source when station is no longer covered", () => {
                setOwnStations(OWN_STATION);
                useStationsStore.setState({
                    defaultSource: OWN_STATION,
                    positionDefaultSources: [],
                });

                act(() => {
                    setStations([{id: OWN_STATION, own: false}]);
                });

                expect(useStationsStore.getState().defaultSource).toBeUndefined();
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: false});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: true, own: false});
            });

            it("resets temporary source when station is no longer covered", () => {
                setOwnStations(OWN_STATION);
                useStationsStore.setState({temporarySource: OWN_STATION});

                act(() => {
                    setStations([{id: OWN_STATION, own: false}]);
                });

                expect(useStationsStore.getState().defaultSource).toBeUndefined();
                expect(useStationsStore.getState().temporarySource).toBeUndefined();
                let result = renderHook(() => useStationKeyInteraction(OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: false, own: false});
                result = renderHook(() => useStationKeyInteraction(OTHER_OWN_STATION));
                expectInteraction(result, {color: "gray", disabled: true, own: false});
            });

            it("chooses next available default source when original station is no longer covered", () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);
                useStationsStore.setState({
                    defaultSource: OWN_STATION,
                    positionDefaultSources: [OWN_STATION, OTHER_OWN_STATION],
                });

                act(() => {
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

            it("promotes temporary source to default when original default station is no longer covered", () => {
                setOwnStations(OWN_STATION, OTHER_OWN_STATION);
                useStationsStore.setState({
                    defaultSource: OWN_STATION,
                    temporarySource: OTHER_OWN_STATION,
                    positionDefaultSources: [OWN_STATION, OTHER_OWN_STATION],
                });

                act(() => {
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
});
