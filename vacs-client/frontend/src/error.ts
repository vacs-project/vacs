import {invoke, InvokeArgs, isTauri, RemoteCommand} from "./transport";
import {useErrorOverlayStore} from "./stores/error-overlay-store.ts";
import {CallId} from "./types/generic.ts";

export type Error = {
    title: string;
    detail: string;
    isNonCritical: boolean;
    timeoutMs?: number;
};

export type CallError = {
    callId: CallId;
    reason: string;
};

export async function invokeSafe<T>(cmd: RemoteCommand, args?: InvokeArgs): Promise<T | undefined> {
    try {
        return await invoke<T>(cmd, args);
    } catch (e) {
        openErrorOverlayFromUnknown(e);
    }
}

export async function invokeStrict<T>(cmd: RemoteCommand, args?: InvokeArgs): Promise<T> {
    try {
        return await invoke<T>(cmd, args);
    } catch (e) {
        openErrorOverlayFromUnknown(e);
        throw e;
    }
}

export function openErrorOverlayFromUnknown(e: unknown) {
    const openErrorOverlay = useErrorOverlayStore.getState().open;

    if (isError(e)) {
        openErrorOverlay(e.title, e.detail, e.isNonCritical, e.timeoutMs);
    } else {
        logError(JSON.stringify(e));
        openErrorOverlay("Unexpected error", "An unknown error occurred", false);
    }
}

export function isError(err: unknown): err is Error {
    if (typeof err !== "object" || err === null) {
        return false;
    }

    const maybeError = err as Record<string, unknown>;

    return (
        typeof maybeError.title === "string" &&
        typeof maybeError.detail === "string" &&
        (maybeError.timeout_ms === undefined || typeof maybeError.timeout_ms === "number")
    );
}

export function safeSerialize(value: unknown): unknown {
    try {
        if (value instanceof Error) {
            return {
                name: value.name,
                message: value.message,
                stack: value.stack,
                cause: (value as any).cause, // eslint-disable-line @typescript-eslint/no-explicit-any
            };
        }
        return JSON.parse(JSON.stringify(value));
    } catch {
        return String(value);
    }
}

export function logError(msg: string) {
    if (isTauri) {
        import("@tauri-apps/plugin-log").then(mod => void mod.error(msg));
    } else {
        console.error(msg);
    }
}
