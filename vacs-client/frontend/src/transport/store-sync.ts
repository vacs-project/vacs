import {syncBlink} from "../stores/blink-store.ts";
import {type CallListItem, useCallListStore} from "../stores/call-list-store.ts";
import {CallDisplay, useCallStore} from "../stores/call-store.ts";
import {PlaybackStatus, usePlaybackStore} from "../stores/playback-store.ts";
import {useRadioStore} from "../stores/radio-store.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import {useStationsStore} from "../stores/stations-store.ts";
import {PlaybackDeviceType} from "../types/audio.ts";
import type {ClientPageConfig} from "../types/client.ts";
import type {CallId, StationId} from "../types/generic.ts";
import type {CallConfig, ClockMode, CplMode, RemoteStatus} from "../types/settings.ts";
import type {RadioConfigWithLabels, TransmitConfigWithLabels} from "../types/transmit.ts";
import {invoke, isRemote, isTauri, listen} from "./index.ts";
import {Call} from "../types/call.ts";

// TODO: Update entire remote for conference calls

type StationsSync = {
    defaultSource: StationId | undefined;
    temporarySource: StationId | undefined;
};

type CallSync = {
    prio: boolean;
    callDisplay: CallDisplay | undefined | null;
    incomingCalls: Call[];
};

type CallListSync = {
    callList: [CallId, CallListItem][];
};

type SettingsSync = {
    callConfig: CallConfig;
    selectedClientPageConfig: ClientPageConfig & {name: string};
    clockMode: ClockMode;
    cplMode: CplMode;
    playbackEnabled: boolean;
    transmitConfig: TransmitConfigWithLabels | undefined;
    radioConfig: RadioConfigWithLabels | undefined;
};

type RadioSync = {
    cpl: boolean;
};

type PlaybackSync = {
    selected: number;
    status: PlaybackStatus | undefined;
    playbackDevice: PlaybackDeviceType;
    openInstanceIds: string[];
};

type SyncMap = {
    stations: StationsSync;
    call: CallSync;
    callList: CallListSync;
    settings: SettingsSync;
    radio: RadioSync;
    playback: PlaybackSync;
};

type SyncStoreName = keyof SyncMap;

type SyncPayload = {
    [K in SyncStoreName]: {store: K; state: SyncMap[K]; sourceId: string};
}[SyncStoreName];

function createInstanceId(): string {
    if (globalThis.crypto?.randomUUID) {
        return globalThis.crypto.randomUUID();
    }

    // Fallback for older browsers: good enough for a handful of instances.
    return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`;
}

// Unique ID for this client instance so we can ignore our own broadcasts.
export const INSTANCE_ID = createInstanceId();

// set to `true` while applying remote-sourced state to prevent re-broadcast
let applying = false;

/**
 * Runs `fn` with sync re-broadcast suppressed. Must wrap any store writes whose
 * values originate from another instance (incoming syncs, snapshot hydration),
 * otherwise they are echoed back and clobber the other instance's stores.
 */
export function withSyncSuppressed(fn: () => void): void {
    const prev = applying;
    applying = true;
    try {
        fn();
    } finally {
        applying = prev;
    }
}

function deepEqual(a: unknown, b: unknown): boolean {
    if (a === b) return true;
    if (a === null || b === null || typeof a !== "object" || typeof b !== "object") return false;
    if (Array.isArray(a) !== Array.isArray(b)) return false;
    if (Array.isArray(a) && Array.isArray(b)) {
        return a.length === b.length && a.every((v, i) => deepEqual(v, b[i]));
    }
    const aKeys = Object.keys(a);
    const bKeys = Object.keys(b);
    return (
        aKeys.length === bKeys.length &&
        aKeys.every(k =>
            deepEqual((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]),
        )
    );
}

function subscribeFields<K extends SyncStoreName, S>(
    store: {
        getState: () => S;
        subscribe: (listener: (state: S, prevState: S) => void) => () => void;
    },
    name: K,
    select: (state: S) => SyncMap[K],
    skipSync?: (next: S, prev: S) => boolean,
): () => void {
    return store.subscribe((nextState, prevState) => {
        if (applying || skipSync?.(nextState, prevState)) return;

        const next = select(nextState);
        const prev = select(prevState);

        if (deepEqual(next, prev)) return;

        void invoke("remote_broadcast_store_sync", {
            store: name,
            state: next,
            sourceId: INSTANCE_ID,
        });
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
                actions: {setPrio},
            } = useCallStore.getState();
            const {prio, callDisplay} = payload.state;
            setPrio(prio);

            if (callDisplay !== null) {
                useCallStore.setState({callDisplay});
                syncBlink();
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
                cplMode: payload.state.cplMode,
                playbackEnabled: payload.state.playbackEnabled,
                transmitConfig: payload.state.transmitConfig,
                radioConfig: payload.state.radioConfig,
            });
            break;
        }
        case "radio": {
            useRadioStore.setState({cpl: payload.state.cpl});
            syncBlink();
            break;
        }
        case "playback": {
            const {selected, status, playbackDevice, openInstanceIds} = payload.state;

            usePlaybackStore.setState({
                selected,
                status,
                playbackDevice,
                openInstanceIds,
            });

            syncBlink();
            break;
        }
    }
}

export function setupStoreSync(): () => void {
    let teardown: (() => void) | undefined;
    let unlistenStatus: (() => void) | undefined;

    const start = () => {
        teardown ??= startSync();
    };
    const stop = () => {
        teardown?.();
        teardown = undefined;
    };

    if (isRemote()) {
        start();
    } else {
        void invoke<boolean>("remote_is_enabled")
            .then(enabled => {
                if (enabled) start();
            })
            .catch(() => {});

        // Arm/disarm syncing when the remote server is enabled or disabled at
        // runtime; the initial query alone would miss later settings changes.
        void listen<RemoteStatus>("remote:status", event => {
            if (event.payload.listening) {
                start();
            } else {
                stop();
            }
        }).then(fn => (unlistenStatus = fn));
    }

    return () => {
        stop();
        unlistenStatus?.();
    };
}

function startSync(): () => void {
    const unlistenFns: (() => void)[] = [];

    void listen<SyncPayload>("store:sync", event => {
        if (event.payload.sourceId === INSTANCE_ID) return;
        withSyncSuppressed(() => applySync(event.payload));
    }).then(fn => unlistenFns.push(fn));

    if (isTauri) {
        void listen("store:sync:request", () => {
            broadcastAllStoreState();
        }).then(fn => unlistenFns.push(fn));
    }

    unlistenFns.push(
        subscribeFields(useStationsStore, "stations", s => ({
            defaultSource: s.defaultSource,
            temporarySource: s.temporarySource,
        })),
    );

    unlistenFns.push(
        subscribeFields(useCallStore, "call", s => ({
            prio: s.prio,
            incomingCalls: s.incomingCalls,
            callDisplay:
                s.callDisplay === undefined ||
                s.callDisplay.type === "outgoing" ||
                s.callDisplay.type === "error" ||
                s.callDisplay.type === "rejected"
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
            cplMode: s.cplMode,
            playbackEnabled: s.playbackEnabled,
            transmitConfig: s.transmitConfig,
            radioConfig: s.radioConfig,
        })),
    );

    unlistenFns.push(
        subscribeFields(useRadioStore, "radio", s => ({
            cpl: s.cpl,
        })),
    );

    unlistenFns.push(
        subscribeFields(
            usePlaybackStore,
            "playback",
            s => ({
                selected: s.selected,
                status: s.status,
                playbackDevice: s.playbackDevice,
                openInstanceIds: s.openInstanceIds,
            }),
            (nextState, prevState) => {
                if (nextState.status === undefined || prevState.status === undefined) return false;

                const {progress: _nextProgress, ...nextStatusWithoutProgress} = nextState.status;
                const {progress: _prevProgress, ...prevStatusWithoutProgress} = prevState.status;

                const next = {...nextState, status: nextStatusWithoutProgress};
                const prev = {...prevState, status: prevStatusWithoutProgress};

                return deepEqual(prev, next);
            },
        ),
    );

    return () => {
        unlistenFns.forEach(fn => fn());
    };
}

function broadcastAllStoreState() {
    const broadcast = <K extends SyncStoreName>(name: K, state: SyncMap[K]) => {
        void invoke("remote_broadcast_store_sync", {store: name, state, sourceId: INSTANCE_ID});
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
        incomingCalls: call.incomingCalls,
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
        cplMode: settings.cplMode,
        playbackEnabled: settings.playbackEnabled,
        transmitConfig: settings.transmitConfig,
        radioConfig: settings.radioConfig,
    });

    const radio = useRadioStore.getState();
    broadcast("radio", {
        cpl: radio.cpl,
    });

    const playback = usePlaybackStore.getState();
    broadcast("playback", {
        selected: playback.selected,
        status: playback.status,
        playbackDevice: playback.playbackDevice,
        openInstanceIds: playback.openInstanceIds,
    });
}
