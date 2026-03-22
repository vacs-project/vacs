import type {EventCallback, InvokeArgs, RemoteCommand, RemoteEvent, UnlistenFn} from "./types.ts";
import {SessionStateSnapshot} from "./hydrate.ts";
import {useErrorOverlayStore} from "../stores/error-overlay-store.ts";

const PING_INTERVAL_MS = 5_000;
const PONG_TIMEOUT_MS = 5_000;
const RECONNECT_DELAY_MS = 2_000;
const DISCONNECT_ERROR_TITLE = "Remote disconnected";

type PendingRequest = {
    resolve: (value: unknown) => void;
    reject: (reason: unknown) => void;
};

type WsMessage =
    | {type: "response"; id: string; ok: true; data: unknown}
    | {type: "response"; id: string; ok: false; error: unknown}
    | {type: "event"; name: RemoteEvent; payload: unknown}
    | {type: "pong"};

class RemoteTransport {
    private ws: WebSocket | null = null;
    private pendingRequests = new Map<string, PendingRequest>();
    private eventListeners = new Map<RemoteEvent, Set<EventCallback<unknown>>>();
    private messageId = 0;
    private connectPromise: Promise<void> | null = null;
    private pingIntervalId: ReturnType<typeof setInterval> | undefined;
    private pongTimeoutId: ReturnType<typeof setTimeout> | undefined;

    async connect(): Promise<void> {
        if (this.connectPromise) return this.connectPromise;

        this.connectPromise = new Promise<void>((resolve, reject) => {
            const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
            const wsUrl = `${protocol}//${window.location.host}/ws`;
            this.ws = new WebSocket(wsUrl);

            this.ws.onopen = () => {
                console.log("[remote] WebSocket connected");
                useErrorOverlayStore.getState().closeIfTitle(DISCONNECT_ERROR_TITLE);
                for (const event of this.eventListeners.keys()) {
                    this.ws!.send(JSON.stringify({type: "subscribe", event}));
                }
                this.startPing();
                resolve();
                void this.hydrateFromSnapshot();
            };

            this.ws.onmessage = ev => {
                const msg: WsMessage = JSON.parse(ev.data as string);
                this.handleMessage(msg);
            };

            this.ws.onclose = () => {
                console.warn("[remote] WebSocket closed, will reconnect...");
                this.stopPing();
                this.connectPromise = null;
                for (const pending of this.pendingRequests.values()) {
                    pending.reject(new Error("WebSocket connection lost"));
                }
                this.pendingRequests.clear();
                useErrorOverlayStore
                    .getState()
                    .open(
                        DISCONNECT_ERROR_TITLE,
                        "Disconnected from vacs, reconnecting...",
                        false,
                        undefined,
                        false,
                    );
                setTimeout(() => void this.connect(), RECONNECT_DELAY_MS);
            };

            this.ws.onerror = err => {
                console.error("[remote] WebSocket error", err);
                reject(err);
            };
        });

        return this.connectPromise;
    }

    private handleMessage(msg: WsMessage) {
        switch (msg.type) {
            case "pong": {
                clearTimeout(this.pongTimeoutId);
                this.pongTimeoutId = undefined;
                break;
            }
            case "response": {
                const pending = this.pendingRequests.get(msg.id);
                if (pending) {
                    this.pendingRequests.delete(msg.id);
                    if (msg.ok) {
                        pending.resolve(msg.data);
                    } else {
                        pending.reject(msg.error);
                    }
                }
                break;
            }
            case "event": {
                const listeners = this.eventListeners.get(msg.name);
                if (listeners) {
                    for (const cb of listeners) {
                        try {
                            cb({payload: msg.payload as never});
                        } catch (e) {
                            console.error(`[remote] Event listener error for %s:`, msg.name, e);
                        }
                    }
                }
                break;
            }
        }
    }

    async invoke<T>(cmd: RemoteCommand, args?: InvokeArgs): Promise<T> {
        await this.connect();

        const id = String(++this.messageId);
        return new Promise<T>((resolve, reject) => {
            this.pendingRequests.set(id, {
                resolve: resolve as (v: unknown) => void,
                reject,
            });
            this.ws!.send(JSON.stringify({type: "invoke", id, cmd, args: args ?? {}}));
        });
    }

    listen<T>(event: RemoteEvent, callback: EventCallback<T>): UnlistenFn {
        let listeners = this.eventListeners.get(event);
        if (!listeners) {
            listeners = new Set();
            this.eventListeners.set(event, listeners);
        }
        const cb = callback as EventCallback<unknown>;
        listeners.add(cb);

        if (this.ws?.readyState === WebSocket.OPEN) {
            this.ws.send(JSON.stringify({type: "subscribe", event}));
        }

        return () => {
            listeners!.delete(cb);
            if (listeners!.size === 0) {
                this.eventListeners.delete(event);
                if (this.ws?.readyState === WebSocket.OPEN) {
                    this.ws.send(JSON.stringify({type: "unsubscribe", event}));
                }
            }
        };
    }

    private async hydrateFromSnapshot() {
        try {
            const snapshot = await this.invoke<SessionStateSnapshot>("remote_get_session_state");
            const {hydrateStores} = await import("./hydrate.ts");
            hydrateStores(snapshot);
            await this.invoke("remote_request_store_sync");
        } catch (e) {
            console.error("[remote] Failed to hydrate stores from snapshot:", e);
        }
    }

    private startPing() {
        this.stopPing();
        this.pingIntervalId = setInterval(() => {
            if (this.ws?.readyState !== WebSocket.OPEN) return;
            this.ws.send(JSON.stringify({type: "ping"}));
            this.pongTimeoutId = setTimeout(() => {
                console.warn("[remote] Pong timeout, closing WebSocket");
                this.ws?.close();
            }, PONG_TIMEOUT_MS);
        }, PING_INTERVAL_MS);
    }

    private stopPing() {
        clearInterval(this.pingIntervalId);
        this.pingIntervalId = undefined;
        clearTimeout(this.pongTimeoutId);
        this.pongTimeoutId = undefined;
    }
}

let remoteTransport: RemoteTransport | undefined;

export function getRemoteTransport(): RemoteTransport {
    if (!remoteTransport) {
        remoteTransport = new RemoteTransport();
        void remoteTransport.connect();
    }
    return remoteTransport;
}
