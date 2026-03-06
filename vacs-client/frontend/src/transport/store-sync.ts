import {invoke, isRemote, listen} from "./index.ts";
import {useStationsStore} from "../stores/stations-store.ts";
import {type CallDisplayType, useCallStore} from "../stores/call-store.ts";
import {type CallListItem, useCallListStore} from "../stores/call-list-store.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import type {CallId, StationId} from "../types/generic.ts";
import type {UnlistenFn} from "./types.ts";
import type {ClientPageConfig} from "../types/client.ts";

type StationsSync = {
    defaultSource: StationId | undefined;
    temporarySource: StationId | undefined;
};

type CallSync = {
    prio: boolean;
    callDisplay: {type: CallDisplayType; call: {prio: boolean}} | null;
};

type CallListSync = {
    callList: [CallId, CallListItem][];
};

type SettingsSync = {
    selectedClientPageConfig: ClientPageConfig & {name: string};
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
            const {prio, callDisplay} = payload.state;
            const actions = useCallStore.getState().actions;
            actions.setPrio(prio);

            const current = useCallStore.getState().callDisplay;

            if (callDisplay == null) {
                if (current != null) actions.endCall();
            } else {
                useCallStore.setState({callDisplay: callDisplay as never});

                const incomingCount = useCallStore.getState().incomingCalls.length;
                const shouldBlink =
                    incomingCount > 0 ||
                    (callDisplay.type === "outgoing" && callDisplay.call.prio) ||
                    callDisplay.type === "rejected" ||
                    callDisplay.type === "error";

                const blinkTimeoutId = useCallStore.getState().blinkTimeoutId;
                if (shouldBlink && blinkTimeoutId === undefined) {
                    const toggleBlink = (blink: boolean) => {
                        const timeoutId = setTimeout(() => {
                            toggleBlink(!blink);
                        }, 500);
                        useCallStore.setState({blinkTimeoutId: timeoutId, blink});
                    };
                    toggleBlink(true);
                } else if (!shouldBlink && blinkTimeoutId !== undefined) {
                    clearTimeout(blinkTimeoutId);
                    useCallStore.setState({blink: false, blinkTimeoutId: undefined});
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
                selectedClientPageConfig: payload.state.selectedClientPageConfig,
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
    const unsubs: (() => void)[] = [];

    unsubs.push(
        subscribeFields(useStationsStore, "stations", s => ({
            defaultSource: s.defaultSource,
            temporarySource: s.temporarySource,
        })),
    );

    unsubs.push(
        subscribeFields(useCallStore, "call", s => ({
            prio: s.prio,
            callDisplay: s.callDisplay ?? null,
        })),
    );

    unsubs.push(
        subscribeFields(useCallListStore, "callList", s => ({
            callList: Array.from(s.callList.entries()),
        })),
    );

    unsubs.push(
        subscribeFields(useSettingsStore, "settings", s => ({
            selectedClientPageConfig: s.selectedClientPageConfig,
        })),
    );

    let unlistenSync: UnlistenFn | undefined;
    const unlistenPromise = listen<SyncPayload>("store:sync", event => {
        applying = true;
        try {
            applySync(event.payload);
        } finally {
            applying = false;
        }
    });
    void unlistenPromise.then(fn => {
        unlistenSync = fn;
    });

    return () => {
        unsubs.forEach(fn => fn());
        unlistenSync?.();
    };
}
