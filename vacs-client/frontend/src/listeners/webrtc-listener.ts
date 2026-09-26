import {listen, UnlistenFn} from "../transport";
import {useCallStore} from "../stores/call-store.ts";
import {CallError} from "../error.ts";
import {CallId, ClientId} from "../types/generic.ts";

type WebrtcUpdateEvent = {
    callId: CallId;
    peerId: ClientId;
};

export function setupWebrtcListeners() {
    const {errorTargets, setConnectionState} = useCallStore.getState().actions;

    const unlistenFns: Promise<UnlistenFn>[] = [];

    const init = () => {
        unlistenFns.push(
            listen<WebrtcUpdateEvent>("webrtc:call-connected", event => {
                setConnectionState(event.payload.callId, event.payload.peerId, "connected");
            }),
            listen<WebrtcUpdateEvent>("webrtc:call-disconnected", event => {
                setConnectionState(event.payload.callId, event.payload.peerId, "disconnected");
            }),
            listen<WebrtcUpdateEvent>("webrtc:call-degraded", event => {
                setConnectionState(event.payload.callId, event.payload.peerId, "degraded");
            }),
            listen<WebrtcUpdateEvent>("webrtc:call-reconnecting", event => {
                setConnectionState(event.payload.callId, event.payload.peerId, "connecting");
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
