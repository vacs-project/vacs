import {invoke, isRemote, isTauri, listen} from "./index.ts";
import {useStationsStore} from "../stores/stations-store.ts";
import {CallDisplay, shouldStopBlinking, useCallStore} from "../stores/call-store.ts";
import {type CallListItem, useCallListStore} from "../stores/call-list-store.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import type {CallId, StationId} from "../types/generic.ts";
import type {ClientPageConfig} from "../types/client.ts";
import type {CallConfig, ClockMode} from "../types/settings.ts";

type StationsSync = {
    defaultSource: StationId | undefined;
    temporarySource: StationId | undefined;
};

type CallSync = {
    prio: boolean;
    callDisplay: CallDisplay | undefined | null;
};

type CallListSync = {
    callList: [CallId, CallListItem][];
};

type SettingsSync = {
    callConfig: CallConfig;
    selectedClientPageConfig: ClientPageConfig & {name: string};
    clockMode: ClockMode;
};

type SyncMap = {
    stations: StationsSync;
    call: CallSync;
    callList: CallListSync;
    settings: SettingsSync;
};

type SyncStoreName = keyof SyncMap;

type SyncPayload = {[K in SyncStoreName]: {store: K; state: SyncMap[K]}}[SyncStoreName];

// set to `true` while applying an incoming sync to prevent re-broadcast
let applying = false;

function subscribeFields<K extends SyncStoreName, S>(
    store: {getState: () => S; subscribe: (fn: (state: S) => void) => () => void},
    name: K,
    select: (state: S) => SyncMap[K],
): () => void {
    let prev = JSON.stringify(select(store.getState()));

    return store.subscribe(state => {
        const next = JSON.stringify(select(state));
        if (next === prev) return;
        prev = next;
        if (applying) return;
        void invoke("remote_broadcast_store_sync", {store: name, state: JSON.parse(next)});
    });
}

function applySync(payload: SyncPayload) {
    switch (payload.store) {
        case "stations": {
            const {defaultSource, temporarySource} = payload.state;
            const actions = useStationsStore.getState();
            actions.setDefaultSource(defaultSource);
            actions.setTemporarySource(temporarySource);
            break;
        }
        case "call": {
            const {
                incomingCalls,
                actions: {setPrio, startBlink, stopBlink},
            } = useCallStore.getState();
            const {prio, callDisplay} = payload.state;
            setPrio(prio);

            if (callDisplay !== null) {
                useCallStore.setState({callDisplay});

                const shouldStartBlink = !shouldStopBlinking(incomingCalls.length, callDisplay);
                if (shouldStartBlink) {
                    startBlink();
                } else {
                    stopBlink();
                }
            }
            break;
        }
        case "callList": {
            useCallListStore.setState({callList: new Map(payload.state.callList)});
            break;
        }
        case "settings": {
            useSettingsStore.setState({
                callConfig: payload.state.callConfig,
                selectedClientPageConfig: payload.state.selectedClientPageConfig,
                clockMode: payload.state.clockMode,
            });
            break;
        }
    }
}

export function setupStoreSync(): () => void {
    let teardown: (() => void) | undefined;

    const shouldEnable: Promise<boolean> = isRemote()
        ? Promise.resolve(true)
        : invoke<boolean>("remote_is_enabled").catch(() => false);

    void shouldEnable.then(enabled => {
        if (!enabled) return;
        teardown = startSync();
    });

    return () => {
        teardown?.();
    };
}

function startSync(): () => void {
    const unlistenFns: (() => void)[] = [];

    unlistenFns.push(
        subscribeFields(useStationsStore, "stations", s => ({
            defaultSource: s.defaultSource,
            temporarySource: s.temporarySource,
        })),
    );

    unlistenFns.push(
        subscribeFields(useCallStore, "call", s => ({
            prio: s.prio,
            callDisplay:
                s.callDisplay === undefined || s.callDisplay.type === "outgoing"
                    ? s.callDisplay
                    : null,
        })),
    );

    unlistenFns.push(
        subscribeFields(useCallListStore, "callList", s => ({
            callList: Array.from(s.callList.entries()),
        })),
    );

    unlistenFns.push(
        subscribeFields(useSettingsStore, "settings", s => ({
            callConfig: s.callConfig,
            selectedClientPageConfig: s.selectedClientPageConfig,
            clockMode: s.clockMode,
        })),
    );

    const unlistenSync = listen<SyncPayload>("store:sync", event => {
        applying = true;
        try {
            applySync(event.payload);
        } finally {
            applying = false;
        }
    });
    unlistenFns.push(() => unlistenSync.then(fn => fn()));

    if (isTauri) {
        const unlistenSyncRequest = listen("store:sync:request", () => {
            broadcastAllStoreState();
        });
        unlistenFns.push(() => unlistenSyncRequest.then(fn => fn()));
    }

    return () => {
        unlistenFns.forEach(fn => fn());
    };
}

function broadcastAllStoreState() {
    const broadcast = <K extends SyncStoreName>(name: K, state: SyncMap[K]) => {
        void invoke("remote_broadcast_store_sync", {store: name, state});
    };

    const stations = useStationsStore.getState();
    broadcast("stations", {
        defaultSource: stations.defaultSource,
        temporarySource: stations.temporarySource,
    });

    const call = useCallStore.getState();
    broadcast("call", {
        prio: call.prio,
        callDisplay: call.callDisplay,
    });

    const callList = useCallListStore.getState();
    broadcast("callList", {
        callList: Array.from(callList.callList.entries()),
    });

    const settings = useSettingsStore.getState();
    broadcast("settings", {
        callConfig: settings.callConfig,
        selectedClientPageConfig: settings.selectedClientPageConfig,
        clockMode: settings.clockMode,
    });
}
