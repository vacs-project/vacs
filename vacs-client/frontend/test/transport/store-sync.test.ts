import {afterEach, describe, expect, it, vi} from "vitest";

type EventHandler = (event: {payload: unknown}) => void;

const {invoke, listen, handlers} = vi.hoisted(() => {
    const handlers = new Map<string, EventHandler>();
    return {
        handlers,
        invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
            Promise.resolve(undefined),
        ),
        listen: vi.fn<(event: string, handler: EventHandler) => Promise<() => void>>(
            (event, handler) => {
                handlers.set(event, handler);
                return Promise.resolve(() => {
                    handlers.delete(event);
                });
            },
        ),
    };
});

vi.mock("../../src/transport", () => ({
    invoke,
    listen,
    isTauri: false,
    isRemote: () => true,
}));

import {useSettingsStore} from "../../src/stores/settings-store.ts";
import {useCallStore} from "../../src/stores/call-store.ts";
import {hydrateStores, SessionStateSnapshot} from "../../src/transport/hydrate.ts";
import {setupStoreSync} from "../../src/transport/store-sync.ts";
import {flushMicrotasks, makeTestCall, makeTestCallDisplay} from "../util.ts";
import type {CallId, ClientId} from "../../src/types/generic.ts";

const snapshot: SessionStateSnapshot = {
    connectionState: "disconnected",
    sessionInfo: null,
    defaultCallSources: [],
    stations: [],
    clients: [],
    clientId: null,
    // Differs from the settings store defaults, so hydration actually changes state.
    callConfig: {
        highlightIncomingCallTarget: false,
        enablePriorityCalls: true,
        enableCallStartSound: false,
        enableCallEndSound: true,
        enableParticipantJoinedSound: false,
        enableParticipantLeftSound: true,
        useDefaultCallSources: true,
        forceRelay: false,
    },
    clientPageSettings: {selected: undefined, configs: {}},
    capabilities: {
        alwaysOnTop: true,
        keybindListener: true,
        keybindEmitter: true,
        joystick: true,
        playback: true,
        platform: "Windows",
    },
};

const receiveSync = (state: Record<string, unknown>) =>
    handlers.get("store:sync")!({payload: {store: "call", state, sourceId: "desktop"}});

function findStoreSyncCallback() {
    const call = listen.mock.calls.find(([event]) => event === "store:sync");
    if (call === undefined) throw new Error("store:sync was never registered");
    return call[1];
}

describe("store sync", () => {
    afterEach(() => {
        useSettingsStore.setState({playbackEnabled: false});
        vi.clearAllMocks();
        useCallStore.getState().actions.reset();
        useCallStore.getState().actions.setPrio(false);
    });

    it("does not re-broadcast store state while hydrating from a snapshot", async () => {
        const teardown = setupStoreSync();
        await flushMicrotasks();
        invoke.mockClear();

        hydrateStores(snapshot);

        // Hydration must have taken effect...
        expect(useSettingsStore.getState().callConfig).toEqual(snapshot.callConfig);
        // ...but must not be echoed back to the desktop, where it would clobber
        // the settings store (transmitConfig/radioConfig -> undefined).
        expect(invoke).not.toHaveBeenCalledWith("remote_broadcast_store_sync", expect.anything());

        teardown();
    });

    it("still broadcasts local store changes after hydration", async () => {
        const teardown = setupStoreSync();
        await flushMicrotasks();
        hydrateStores(snapshot);
        invoke.mockClear();

        useSettingsStore.getState().setPlaybackEnabled(true);

        expect(invoke).toHaveBeenCalledWith(
            "remote_broadcast_store_sync",
            expect.objectContaining({
                store: "settings",
                state: expect.objectContaining({playbackEnabled: true}),
            }),
        );

        teardown();
    });

    it("applies an inbound settings sync without echoing it back", async () => {
        useSettingsStore.setState({playbackEnabled: false});
        const teardown = setupStoreSync();
        await flushMicrotasks();
        invoke.mockClear();

        const settings = useSettingsStore.getState();
        findStoreSyncCallback()({
            payload: {
                store: "settings",
                sourceId: "other",
                state: {
                    callConfig: settings.callConfig,
                    selectedClientPageConfig: settings.selectedClientPageConfig,
                    clockMode: settings.clockMode,
                    cplMode: settings.cplMode,
                    transmitConfig: settings.transmitConfig,
                    radioConfig: settings.radioConfig,
                    playbackEnabled: true,
                },
            },
        });

        expect(useSettingsStore.getState().playbackEnabled).toBe(true);
        expect(invoke).not.toHaveBeenCalledWith("remote_broadcast_store_sync", expect.anything());

        teardown();
    });

    it("sets the max conference size from the snapshot's session info", () => {
        hydrateStores({
            ...snapshot,
            sessionInfo: {
                client: {
                    id: "client0" as ClientId,
                    positionId: undefined,
                    displayName: "EDDF_TWR",
                    frequency: "119.900",
                },
                profile: {type: "unchanged"},
                defaultCallSources: [],
                maxConfSize: 4,
            },
        });

        expect(useCallStore.getState().maxConferenceSize).toBe(4);
    });

    it("bootstraps the full call state from a sync re-broadcast", async () => {
        const teardown = setupStoreSync();
        await flushMicrotasks();
        invoke.mockClear();

        const callDisplay = makeTestCallDisplay("accepted");
        const incoming = makeTestCall("incoming", {callId: "call1" as CallId});
        receiveSync({
            prio: true,
            callDisplay,
            incomingCalls: [incoming],
            conferenceState: "active",
        });

        const state = useCallStore.getState();
        expect(state.callDisplay).toEqual(callDisplay);
        expect(state.incomingCalls).toEqual([incoming]);
        expect(state.conferenceState).toBe("active");
        expect(state.prio).toBe(true);
        // Applied state must not be echoed back to the desktop.
        expect(invoke).not.toHaveBeenCalledWith("remote_broadcast_store_sync", expect.anything());

        teardown();
    });

    it("leaves event-driven call state alone on a live sync", async () => {
        const teardown = setupStoreSync();
        await flushMicrotasks();

        const incoming = makeTestCall("incoming", {callId: "call1" as CallId});
        useCallStore.setState({
            callDisplay: makeTestCallDisplay("accepted"),
            incomingCalls: [incoming],
            conferenceState: "active",
        });

        receiveSync({prio: true, callDisplay: null, incomingCalls: null, conferenceState: null});

        const state = useCallStore.getState();
        expect(state.callDisplay?.type).toBe("accepted");
        expect(state.incomingCalls).toEqual([incoming]);
        expect(state.conferenceState).toBe("active");
        expect(state.prio).toBe(true);

        teardown();
    });

    it("does not carry event-driven call state in live broadcasts", async () => {
        const teardown = setupStoreSync();
        await flushMicrotasks();
        invoke.mockClear();

        useCallStore.setState({callDisplay: makeTestCallDisplay("accepted")});

        expect(invoke).toHaveBeenCalledWith("remote_broadcast_store_sync", {
            store: "call",
            state: {prio: false, callDisplay: null, incomingCalls: null, conferenceState: null},
            sourceId: expect.any(String),
        });

        invoke.mockClear();
        useCallStore.setState({
            incomingCalls: [makeTestCall("incoming", {callId: "call1" as CallId})],
            conferenceState: "active",
        });

        expect(invoke).not.toHaveBeenCalledWith(
            "remote_broadcast_store_sync",
            expect.objectContaining({store: "call"}),
        );

        teardown();
    });
});
