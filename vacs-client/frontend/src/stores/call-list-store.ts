import {create} from "zustand/react";
import {CallSource, callSourceToTarget, CallTarget} from "../types/call.ts";
import {CallId, ClientId, PositionId, StationId} from "../types/generic.ts";
import {useShallow} from "zustand/react/shallow";
import {getProfileStationKeysState} from "./profile-store.ts";

export type CallListTarget = {
    target: CallTarget;
    clientId: ClientId | undefined;
};

export type CallListItem = {
    type: "IN" | "OUT";
    time: string;
    name: string;
    targets: CallListTarget[];
    answered: boolean | undefined;
};

export type IncomingCallListEntry = {
    callId: CallId;
    source: CallSource;
};

export type OutgoingCallListEntry = {
    callId: CallId;
    targets: CallTarget[];
};

export type CallListUpdate = {
    targets?: CallListTarget[];
    answered?: boolean;
};

type CallListState = {
    callList: Map<CallId, CallListItem>;
    actions: {
        addIncomingCallListEntry: (entry: IncomingCallListEntry) => void;
        addOutgoingCallListEntry: (entry: OutgoingCallListEntry) => void;
        updateCallListEntry: (
            callId: CallId,
            update: CallListUpdate | ((state: CallListItem) => CallListUpdate),
        ) => void;
        clearCallList: () => void;
    };
};

export const useCallListStore = create<CallListState>()((set, get) => ({
    callList: new Map(),
    actions: {
        addIncomingCallListEntry: (entry: IncomingCallListEntry) => {
            const callList = new Map(get().callList);

            callList.set(entry.callId, {
                type: "IN",
                time: now(),
                name: callListName(
                    entry.source.stationId,
                    entry.source.positionId,
                    entry.source.clientId,
                ),
                targets: [
                    {target: callSourceToTarget(entry.source), clientId: entry.source.clientId},
                ],
                answered: undefined,
            });

            set({callList});
        },
        addOutgoingCallListEntry: (entry: OutgoingCallListEntry) => {
            if (entry.targets.length == 0) return;

            const callList = new Map(get().callList);

            callList.set(entry.callId, {
                type: "OUT",
                time: now(),
                name:
                    entry.targets.length == 1
                        ? callListName(
                              entry.targets[0].station,
                              entry.targets[0].position,
                              entry.targets[0].client,
                          )
                        : "CONF",
                targets: entry.targets.map(target => ({target, clientId: target.client})),
                answered: undefined,
            });

            set({callList});
        },
        updateCallListEntry: (callId, updateOrFn) => {
            const callList = new Map(get().callList);
            const current = callList.get(callId);

            if (current === undefined) return;

            let update: CallListUpdate;
            if (typeof updateOrFn === "function") {
                update = updateOrFn(current);
            } else {
                update = updateOrFn;
            }

            const item = {...current};

            if (update.targets !== undefined && update.targets.length > 0) {
                item.targets = [...update.targets];
            }

            if (item.targets.length > 1) {
                item.name = "CONF";
            }

            const answered = update.answered;
            if (answered !== undefined) {
                item.answered = answered;
            }

            callList.set(callId, item);
            set({callList});
        },
        clearCallList: () => {
            set({callList: new Map()});
        },
    },
}));

function now() {
    return new Date().toLocaleString("de-AT", {
        hour: "2-digit",
        minute: "2-digit",
        timeZone: "UTC",
    });
}

export const useCallListArray = () =>
    useCallListStore(useShallow(state => Array.from(state.callList.values()).reverse()));

export const useLastDialledClientId = () =>
    useCallListStore(state => {
        const calls = Array.from(state.callList.values()).reverse();

        for (const call of calls) {
            if (call.type !== "OUT" || call.targets.length !== 1) continue;

            const clientId = call.targets[0].target.client;
            if (clientId !== undefined) {
                return clientId;
            }
        }
    });

function callListName(
    stationId: StationId | undefined,
    positionId: PositionId | undefined,
    clientId: ClientId | undefined,
): string {
    const stationKeys = getProfileStationKeysState();
    if (stationId !== undefined) {
        const station = stationKeys.find(key => key.stationId === stationId);
        if (station !== undefined) {
            return station.label.join(" ");
        }
        return stationId;
    } else if (positionId !== undefined) {
        return positionId;
    }
    return clientId ?? "";
}
