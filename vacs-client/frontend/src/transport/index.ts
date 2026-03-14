/**
 * Transport abstraction layer for vacs-client.
 *
 * When running inside Tauri (desktop), delegates to the native Tauri IPC.
 * When running in a plain browser (remote control), communicates over a WebSocket
 * to the embedded server in the Tauri app.
 *
 * All application code should import `invoke`, `listen`, etc. from this module
 * instead of directly from `@tauri-apps/api`.
 *
 * @module
 */

export type {RemoteCommand, RemoteEvent, InvokeArgs, EventCallback, UnlistenFn} from "./types.ts";
export {isTauri} from "./types.ts";

import {
    isTauri,
    type EventCallback,
    type InvokeArgs,
    type RemoteCommand,
    type RemoteEvent,
    type UnlistenFn,
} from "./types.ts";
import {ensureTauri, getTauriInvoke, getTauriListen} from "./tauri.ts";
import {getRemoteTransport} from "./remote.ts";

/**
 * Invoke a Tauri command.
 * In native mode delegates to `@tauri-apps/api/core#invoke`.
 * In remote mode sends an RPC message over WebSocket.
 */
export async function invoke<T>(cmd: RemoteCommand, args?: InvokeArgs): Promise<T> {
    if (isTauri) {
        await ensureTauri();
        return getTauriInvoke()(cmd, args) as Promise<T>;
    }
    return getRemoteTransport().invoke<T>(cmd, args);
}

/**
 * Listen for a Tauri event.
 * In native mode delegates to `@tauri-apps/api/event#listen`.
 * In remote mode registers against the WebSocket event stream.
 *
 * Returns an unlisten function (synchronous in remote mode, async in native mode).
 */
export async function listen<T>(
    event: RemoteEvent,
    callback: EventCallback<T>,
): Promise<UnlistenFn> {
    if (isTauri) {
        await ensureTauri();
        return getTauriListen()<T>(event, callback);
    }
    return getRemoteTransport().listen<T>(event, callback);
}

export function isRemote(): boolean {
    return !isTauri;
}
