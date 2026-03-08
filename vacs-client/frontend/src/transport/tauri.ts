import type {EventCallback, InvokeArgs, RemoteCommand, RemoteEvent, UnlistenFn} from "./types.ts";

let tauriInvoke: ((cmd: RemoteCommand, args?: InvokeArgs) => Promise<unknown>) | undefined;
let tauriListen: (<T>(event: RemoteEvent, cb: EventCallback<T>) => Promise<UnlistenFn>) | undefined;

export async function ensureTauri() {
    if (tauriInvoke && tauriListen) return;
    const tauriCore = await import("@tauri-apps/api/core");
    const tauriEvent = await import("@tauri-apps/api/event");
    tauriInvoke = tauriCore.invoke as typeof tauriInvoke;
    tauriListen = tauriEvent.listen as <T>(
        event: RemoteEvent,
        cb: EventCallback<T>,
    ) => Promise<UnlistenFn>;
}

export function getTauriInvoke() {
    return tauriInvoke!;
}

export function getTauriListen() {
    return tauriListen!;
}
