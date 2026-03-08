import {listen, UnlistenFn} from "../transport";
import {useCallStore} from "../stores/call-store.ts";
import {CallError} from "../error.ts";
import {CallId} from "../types/generic.ts";

export function setupWebrtcListeners() {
    const {errorCall, setConnectionState} = useCallStore.getState().actions;

    const unlistenFns: Promise<UnlistenFn>[] = [];

    const init = () => {
        unlistenFns.push(
            listen<CallId>("webrtc:call-connected", event => {
                setConnectionState(event.payload, "connected");
            }),
            listen<CallId>("webrtc:call-disconnected", event => {
                setConnectionState(event.payload, "disconnected");
            }),
            listen<CallError>("webrtc:call-error", event => {
                errorCall(event.payload.callId, event.payload.reason);
            }),
        );
    };

    init();

    return () => {
        unlistenFns.forEach(fn => fn.then(f => f()));
    };
}
