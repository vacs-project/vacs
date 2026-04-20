import {create} from "zustand/react";
import {CallSource, callSourceToTarget, CallTarget} from "../types/call.ts";
import {CallId, ClientId, PositionId, StationId} from "../types/generic.ts";
import {useShallow} from "zustand/react/shallow";
import {getProfileStationKeysState} from "./profile-store.ts";

export type CallListItem = {
    type: "IN" | "OUT";
    time: string;
    name: string;
    target: CallTarget;
    clientId?: ClientId;
    answered: boolean | undefined;
};

export type IncomingCallListEntry = {
    callId: CallId;
    source: CallSource;
};

export type OutgoingCallListEntry = {
    callId: CallId;
    target: CallTarget;
};

export type CallListUpdate = {
    clientId?: ClientId | null;
    answered?: boolean | null;
};

type CallListState = {
    callList: Map<CallId, CallListItem>;
    actions: {
        addIncomingCall: (entry: IncomingCallListEntry) => void;
        addOutgoingCall: (entry: OutgoingCallListEntry) => void;
        updateCall: (
            callId: CallId,
            update: CallListUpdate | ((state: CallListItem) => CallListUpdate),
        ) => void;
        clearCallList: () => void;
    };
};

export const useCallListStore = create<CallListState>()((set, get) => ({
    callList: new Map(),
    actions: {
        addIncomingCall: (entry: IncomingCallListEntry) => {
            const callList = new Map(get().callList);

            callList.set(entry.callId, {
                type: "IN",
                time: now(),
                name: callListName(
                    entry.source.stationId,
                    entry.source.positionId,
                    entry.source.clientId,
                ),
                target: callSourceToTarget(entry.source),
                clientId: entry.source.clientId,
                answered: undefined,
            });

            set({callList});
        },
        addOutgoingCall: (entry: OutgoingCallListEntry) => {
            const callList = new Map(get().callList);

            callList.set(entry.callId, {
                type: "OUT",
                time: now(),
                name: callListName(
                    entry.target.station,
                    entry.target.position,
                    entry.target.client,
                ),
                target: entry.target,
                clientId: entry.target.client,
                answered: undefined,
            });

            set({callList});
        },
        updateCall: (callId, update) => {
            const callList = new Map(get().callList);
            let item = callList.get(callId);

            if (item === undefined) return;

            if (typeof update === "function") {
                update = update(item);
            }

            if (update.clientId !== undefined) {
                item.clientId = update.clientId ?? undefined;
            }

            if (update.answered !== undefined) {
                item.answered = update.answered ?? undefined;
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
            if (call.type === "OUT") {
                return call.clientId;
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
