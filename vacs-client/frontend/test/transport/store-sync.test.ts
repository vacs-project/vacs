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
import {type CallListItem, useCallListStore} from "../../src/stores/call-list-store.ts";
import {useCallStore} from "../../src/stores/call-store.ts";
import {useStationsStore} from "../../src/stores/stations-store.ts";
import {hydrateStores, SessionStateSnapshot} from "../../src/transport/hydrate.ts";
import {setupStoreSync} from "../../src/transport/store-sync.ts";
import {flushMicrotasks, makeTestCall, makeTestCallDisplay} from "../util.ts";
import type {CallId, ClientId, StationId} from "../../src/types/generic.ts";

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

const receiveCallListSync = (callList: [CallId, CallListItem][]) =>
    handlers.get("store:sync")!({
        payload: {store: "callList", state: {callList}, sourceId: "desktop"},
    });

const CALL_LIST_ITEM: CallListItem = {
    type: "OUT",
    time: "12:34",
    name: "LOWI APP",
    targets: [{target: {station: "LOWI_APP" as StationId}, clientId: undefined}],
    answered: true,
};

describe("store sync", () => {
    afterEach(() => {
        vi.clearAllMocks();
        useCallStore.getState().actions.reset();
        useCallStore.getState().actions.setPrio(false);
        useCallListStore.getState().actions.clearCallList();
        useStationsStore.getState().reset();
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

        receiveSync({
            prio: true,
            callDisplay: null,
            incomingCalls: null,
            conferenceState: "active",
        });

        const state = useCallStore.getState();
        expect(state.callDisplay?.type).toBe("accepted");
        expect(state.incomingCalls).toEqual([incoming]);
        expect(state.conferenceState).toBe("active");
        expect(state.prio).toBe(true);

        teardown();
    });

    it("carries only store-driven displays in live broadcasts", async () => {
        const teardown = setupStoreSync();
        await flushMicrotasks();
        invoke.mockClear();

        // Outgoing, incoming and accepted displays come from events on every instance.
        useCallStore.setState({callDisplay: makeTestCallDisplay("outgoing")});
        useCallStore.setState({callDisplay: makeTestCallDisplay("accepted")});
        useCallStore.setState({
            incomingCalls: [makeTestCall("incoming", {callId: "call1" as CallId})],
        });

        expect(invoke).not.toHaveBeenCalledWith(
            "remote_broadcast_store_sync",
            expect.objectContaining({store: "call"}),
        );

        // Conference modify mode is entered locally, so the state is always carried.
        useCallStore.setState({conferenceState: "active"});

        expect(invoke).toHaveBeenCalledWith("remote_broadcast_store_sync", {
            store: "call",
            state: {prio: false, callDisplay: null, incomingCalls: null, conferenceState: "active"},
            sourceId: expect.any(String),
        });

        // A terminal display and its dismissal only exist in the store.
        const rejected = makeTestCallDisplay("rejected");
        useCallStore.setState({callDisplay: rejected});

        expect(invoke).toHaveBeenCalledWith("remote_broadcast_store_sync", {
            store: "call",
            state: {
                prio: false,
                callDisplay: rejected,
                incomingCalls: null,
                conferenceState: "active",
            },
            sourceId: expect.any(String),
        });

        invoke.mockClear();
        useCallStore.setState({callDisplay: undefined});

        const dismissal = invoke.mock.calls.find(
            ([cmd, args]) => cmd === "remote_broadcast_store_sync" && args?.store === "call",
        );
        expect(dismissal?.[1]?.state).toEqual({
            prio: false,
            callDisplay: undefined,
            incomingCalls: null,
            conferenceState: "active",
        });

        teardown();
    });

    it("falls back to the snapshot's default call sources without session info", () => {
        hydrateStores({
            ...snapshot,
            stations: [
                {id: "LOVV_N1" as StationId, own: false},
                {id: "LOVV_N2" as StationId, own: true},
            ],
            defaultCallSources: ["LOVV_N1" as StationId, "LOVV_N2" as StationId],
        });

        expect(useStationsStore.getState().positionDefaultSources).toEqual([
            "LOVV_N1" as StationId,
            "LOVV_N2" as StationId,
        ]);
        expect(useStationsStore.getState().defaultSource).toBe("LOVV_N2" as StationId);
        expect(useCallStore.getState().maxConferenceSize).toBeUndefined();
    });

    it("applies an incoming call list sync as a map", async () => {
        const teardown = setupStoreSync();
        await flushMicrotasks();
        invoke.mockClear();

        receiveCallListSync([["call0" as CallId, CALL_LIST_ITEM]]);

        expect(useCallListStore.getState().callList.get("call0" as CallId)).toEqual(CALL_LIST_ITEM);
        expect(invoke).not.toHaveBeenCalledWith("remote_broadcast_store_sync", expect.anything());

        teardown();
    });

    it("broadcasts a local call list change as entry pairs", async () => {
        const teardown = setupStoreSync();
        await flushMicrotasks();
        invoke.mockClear();

        useCallListStore.getState().actions.addOutgoingCallListEntry({
            callId: "call0" as CallId,
            targets: [{station: "LOWI_APP" as StationId}],
        });

        const broadcast = invoke.mock.calls.find(
            ([cmd, args]) => cmd === "remote_broadcast_store_sync" && args?.store === "callList",
        );
        expect(broadcast?.[1]?.state).toEqual({
            callList: [
                ["call0" as CallId, useCallListStore.getState().callList.get("call0" as CallId)],
            ],
        });

        teardown();
    });

    it("broadcasts a prio change while a live display is up", async () => {
        const teardown = setupStoreSync();
        await flushMicrotasks();
        useCallStore.setState({callDisplay: makeTestCallDisplay("accepted")});
        invoke.mockClear();

        useCallStore.getState().actions.setPrio(true);

        expect(invoke).toHaveBeenCalledWith("remote_broadcast_store_sync", {
            store: "call",
            state: {
                prio: true,
                callDisplay: null,
                incomingCalls: null,
                conferenceState: "inactive",
            },
            sourceId: expect.any(String),
        });

        teardown();
    });
});
