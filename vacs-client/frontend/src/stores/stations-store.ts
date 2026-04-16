import {create} from "zustand/react";
import {StationChange, StationInfo} from "../types/station.ts";
import {useConnectionStore} from "./connection-store.ts";
import {StationId} from "../types/generic.ts";
import {useSettingsStore} from "./settings-store.ts";

type StationsState = {
    stations: Map<StationId, boolean>; // boolean => own
    defaultSource: StationId | undefined;
    temporarySource: StationId | undefined;
    positionDefaultSources: StationId[];
    setStations: (stations: StationInfo[]) => void;
    addStationChanges: (changes: StationChange[]) => void;
    setDefaultSource: (source: StationId | undefined) => void;
    setTemporarySource: (source: StationId | undefined) => void;
    setPositionDefaultSources: (sources: StationId[]) => void;
    getPositionDefaultSource: (
        sources: StationId[],
        stations: Map<StationId, boolean>,
    ) => StationId | undefined;
    reset: () => void;
};

export const useStationsStore = create<StationsState>()((set, get, store) => ({
    stations: new Map(),
    defaultSource: undefined,
    temporarySource: undefined,
    positionDefaultSources: [],
    setStations: stationsList => {
        const stations = new Map(stationsList.map(s => [s.id, s.own]));

        const [defaultSource, temporarySource] = checkStationSourcesAreOwn(stations, get);

        set({stations, defaultSource, temporarySource});
    },
    addStationChanges: changes => {
        const stations = new Map(get().stations);
        const ownPositionId = useConnectionStore.getState().info.positionId;

        for (const change of changes) {
            if (change.online !== undefined) {
                stations.set(change.online.stationId, change.online.positionId === ownPositionId);
            } else if (change.handoff !== undefined) {
                stations.set(
                    change.handoff.stationId,
                    change.handoff.toPositionId === ownPositionId,
                );
            } else if (change.offline !== undefined) {
                stations.delete(change.offline.stationId);
            }
        }

        const [defaultSource, temporarySource] = checkStationSourcesAreOwn(stations, get);

        set({stations, defaultSource, temporarySource});
    },
    setDefaultSource: source => {
        const temporarySource =
            source === get().temporarySource ? undefined : get().temporarySource;
        set({defaultSource: source, temporarySource});
    },
    setTemporarySource: source => set({temporarySource: source}),
    setPositionDefaultSources: sources => {
        const defaultSource = get().getPositionDefaultSource(sources, get().stations);
        const temporarySource =
            get().temporarySource === defaultSource ? undefined : get().temporarySource;
        set({positionDefaultSources: sources, defaultSource, temporarySource});
    },
    getPositionDefaultSource: (sources: StationId[], stations: Map<StationId, boolean>) => {
        if (
            !useSettingsStore.getState().callConfig.useDefaultCallSources ||
            get().defaultSource !== undefined
        ) {
            return get().defaultSource;
        }
        return sources.find(s => stations.get(s));
    },
    reset: () => set(store.getInitialState()),
}));

function checkStationSourcesAreOwn(
    stations: Map<StationId, boolean>,
    get: () => StationsState,
): [StationId | undefined, StationId | undefined] {
    const useDefaultCallSources = useSettingsStore.getState().callConfig.useDefaultCallSources;

    let defaultSource = get().defaultSource;
    if (defaultSource !== undefined && !stations.get(defaultSource)) {
        defaultSource = useDefaultCallSources
            ? get().positionDefaultSources.find(s => stations.get(s))
            : undefined;
    } else if (defaultSource === undefined && useDefaultCallSources) {
        defaultSource = get().positionDefaultSources.find(s => stations.get(s));
    }

    let temporarySource = get().temporarySource;
    if (
        (temporarySource !== undefined && !stations.get(temporarySource)) ||
        defaultSource === temporarySource
    ) {
        temporarySource = undefined;
    }

    return [defaultSource, temporarySource];
}
