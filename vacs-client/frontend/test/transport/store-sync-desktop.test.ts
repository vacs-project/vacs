import {afterEach, describe, expect, it, vi} from "vitest";

type EventHandler = (event: {payload: unknown}) => void;

const {invoke, listen, handlers} = vi.hoisted(() => {
    const handlers = new Map<string, EventHandler>();
    return {
        handlers,
        invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(cmd =>
            Promise.resolve(cmd === "remote_is_enabled" ? true : undefined),
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
    isTauri: true,
    isRemote: () => false,
}));

import {useCallStore} from "../../src/stores/call-store.ts";
import {setupStoreSync} from "../../src/transport/store-sync.ts";
import {flushMicrotasks, makeTestCall, makeTestCallDisplay} from "../util.ts";
import type {CallId} from "../../src/types/generic.ts";

describe("store sync on the desktop", () => {
    afterEach(() => {
        vi.clearAllMocks();
        useCallStore.getState().actions.reset();
        useCallStore.getState().actions.setPrio(false);
    });

    it("re-broadcasts the full call state on a sync request", async () => {
        const teardown = setupStoreSync();
        await flushMicrotasks();

        const callDisplay = makeTestCallDisplay("accepted");
        const incoming = makeTestCall("incoming", {callId: "call1" as CallId});
        useCallStore.setState({callDisplay, incomingCalls: [incoming], conferenceState: "active"});
        invoke.mockClear();

        handlers.get("store:sync:request")!({payload: null});

        expect(invoke).toHaveBeenCalledWith("remote_broadcast_store_sync", {
            store: "call",
            state: {prio: false, callDisplay, incomingCalls: [incoming], conferenceState: "active"},
            sourceId: expect.any(String),
        });

        teardown();
    });
});
