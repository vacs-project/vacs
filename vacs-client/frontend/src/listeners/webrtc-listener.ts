import {listen, UnlistenFn} from "../transport";
import {useCallStore} from "../stores/call-store.ts";
import {CallError} from "../error.ts";
import {CallId} from "../types/generic.ts";

export function setupWebrtcListeners() {
    const {errorTargets, setConnectionState} = useCallStore.getState().actions;

    const unlistenFns: Promise<UnlistenFn>[] = [];

    const init = () => {
        unlistenFns.push(
            listen<CallId>("webrtc:call-connected", event => {
                setConnectionState(event.payload, "connected");
            }),
            listen<CallId>("webrtc:call-disconnected", event => {
                setConnectionState(event.payload, "disconnected");
            }),
            listen<CallId>("webrtc:call-degraded", event => {
                setConnectionState(event.payload, "degraded");
            }),
            listen<CallId>("webrtc:call-reconnecting", event => {
                setConnectionState(event.payload, "connecting");
            }),
            listen<CallError>("webrtc:call-error", event => {
                errorTargets(event.payload);
            }),
        );
    };

    init();

    return () => {
        unlistenFns.forEach(fn => fn.then(f => f()));
    };
}
