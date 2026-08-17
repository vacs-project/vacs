import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({
    invoke,
    listen,
    isTauri: false,
    isRemote: () => true,
}));

import {useSettingsStore} from "../../src/stores/settings-store.ts";
import {hydrateStores, SessionStateSnapshot} from "../../src/transport/hydrate.ts";
import {setupStoreSync} from "../../src/transport/store-sync.ts";

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

// setupStoreSync enables syncing asynchronously; in the app, hydration happens
// long after (a WS round-trip), so tests must let the subscriptions settle first.
function flushMicrotasks(): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, 0));
}

function findStoreSyncCallback() {
    const call = listen.mock.calls.find(([event]) => event === "store:sync");
    if (call === undefined) throw new Error("store:sync was never registered");
    return call[1];
}

describe("store sync", () => {
    afterEach(() => {
        useSettingsStore.setState({playbackEnabled: false});
        vi.clearAllMocks();
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
});
