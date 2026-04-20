import {create} from "zustand/react";
import {invokeStrict} from "../error.ts";
import {useErrorOverlayStore} from "./error-overlay-store.ts";
import {useAuthStore} from "./auth-store.ts";
import {Call, CallSource, CallTarget} from "../types/call.ts";
import {CallId, ClientId, StationId} from "../types/generic.ts";
import {useConnectionStore} from "./connection-store.ts";
import {useCallListStore} from "./call-list-store.ts";
import {useStationsStore} from "./stations-store.ts";
import {shouldStopBlinking, startBlink, stopBlink} from "./blink-store.ts";
import {useRadioStore} from "./radio-store.ts";

export type ConnectionState = "connecting" | "connected" | "disconnected";
export type CallDisplayType = "outgoing" | "accepted" | "rejected" | "error";

export type CallDisplay = {
    type: CallDisplayType;
    call: Call;
    targetClientId?: ClientId;
    errorReason?: string;
    connectionState?: ConnectionState;
};

type CallState = {
    callDisplay?: CallDisplay;
    incomingCalls: Call[];
    prio: boolean;
    actions: {
        setOutgoingCall: (call: Call) => void;
        acceptIncomingCall: (callId: CallId) => void;
        setOutgoingCallAccepted: (calLId: CallId, targetClientId: ClientId) => void;
        endCall: () => void;
        addIncomingCall: (call: Call) => void;
        removeCall: (id: CallId, callEnd?: boolean) => void;
        rejectCall: (id: CallId) => void;
        dismissRejectedCall: () => void;
        errorCall: (id: CallId, reason: string) => void;
        dismissErrorCall: () => void;
        setConnectionState: (id: CallId, connectionState: ConnectionState) => void;
        setPrio: (prio: boolean) => void;
        reset: () => void;
    };
};

export const useCallStore = create<CallState>()((set, get) => ({
    blink: false,
    blinkTimeoutId: undefined,
    callDisplay: undefined,
    incomingCalls: [],
    connecting: false,
    prio: false,
    actions: {
        setOutgoingCall: call => {
            if (call.prio) {
                startBlink();
            }

            set({callDisplay: {type: "outgoing", call, connectionState: undefined}});
        },
        acceptIncomingCall: callId => {
            const incomingCall = get().incomingCalls.find(call => call.callId === callId);
            if (incomingCall === undefined) return;

            const incomingCalls = get().incomingCalls.filter(info => info.callId !== callId);

            if (
                shouldStopBlinking(
                    incomingCalls.length,
                    get().callDisplay,
                    useRadioStore.getState().cpl,
                )
            ) {
                stopBlink();
            }

            answerCallInCallList(callId);

            set({
                callDisplay: {
                    type: "accepted",
                    call: incomingCall,
                    targetClientId: incomingCall.source.clientId,
                    connectionState: "connecting",
                },
                incomingCalls,
            });
        },
        setOutgoingCallAccepted: (callId, targetClientId) => {
            const callDisplay = get().callDisplay;

            if (callDisplay?.type !== "outgoing" || callDisplay.call.callId !== callId) return;

            const nextCallDisplay: CallDisplay = {
                ...callDisplay,
                type: "accepted",
                targetClientId,
                connectionState: "connecting",
            };
            if (
                shouldStopBlinking(
                    get().incomingCalls.length,
                    nextCallDisplay,
                    useRadioStore.getState().cpl,
                )
            ) {
                stopBlink();
            }

            answerCallInCallList(callId, targetClientId);

            set({
                callDisplay: nextCallDisplay,
            });
        },
        endCall: () => {
            if (
                shouldStopBlinking(
                    get().incomingCalls.length,
                    undefined,
                    useRadioStore.getState().cpl,
                )
            ) {
                stopBlink();
            }
            set({callDisplay: undefined});
        },
        addIncomingCall: call => {
            const incomingCalls = get().incomingCalls.filter(info => info.callId !== call.callId);

            startBlink();

            set({incomingCalls: [...incomingCalls, call]});
        },
        removeCall: (callId, callEnd) => {
            const incomingCalls = get().incomingCalls.filter(info => info.callId !== callId);
            let callDisplay = get().callDisplay;

            if (
                callDisplay?.call.callId === callId &&
                callDisplay?.type !== "error" &&
                (!callEnd || callDisplay?.type !== "outgoing")
            ) {
                callDisplay = undefined;
            }

            rejectCallInCallListIfUnanswered(callId);

            if (
                shouldStopBlinking(incomingCalls.length, callDisplay, useRadioStore.getState().cpl)
            ) {
                stopBlink();
                set({incomingCalls: [], callDisplay});
            } else {
                set({incomingCalls, callDisplay});
            }
        },
        rejectCall: callId => {
            const callDisplay = get().callDisplay;

            if (
                callDisplay === undefined ||
                callDisplay.call.callId !== callId ||
                callDisplay.type !== "outgoing"
            ) {
                get().actions.removeCall(callId);
                return;
            }

            rejectCallInCallListIfUnanswered(callId);

            set({
                callDisplay: {type: "rejected", call: callDisplay.call, connectionState: undefined},
            });

            startBlink();
        },
        dismissRejectedCall: () => {
            set({callDisplay: undefined});

            if (
                shouldStopBlinking(
                    get().incomingCalls.length,
                    undefined,
                    useRadioStore.getState().cpl,
                )
            ) {
                stopBlink();
            }
        },
        errorCall: (callId, reason) => {
            const callDisplay = get().callDisplay;

            if (
                callDisplay === undefined ||
                callDisplay.call.callId !== callId ||
                callDisplay.type === "rejected"
            ) {
                get().actions.removeCall(callId);
                return;
            }

            set({
                callDisplay: {
                    type: "error",
                    call: callDisplay.call,
                    errorReason: reason,
                    connectionState: undefined,
                },
            });

            rejectCallInCallListIfUnanswered(callId);

            startBlink();
        },
        dismissErrorCall: () => {
            set({callDisplay: undefined});

            if (
                shouldStopBlinking(
                    get().incomingCalls.length,
                    undefined,
                    useRadioStore.getState().cpl,
                )
            ) {
                stopBlink();
            }
        },
        setConnectionState: (callId, connectionState) => {
            const callDisplay = get().callDisplay;

            if (callDisplay === undefined || callDisplay.call.callId !== callId) {
                return;
            }

            set({callDisplay: {...callDisplay, connectionState}});
        },
        setPrio: prio => set({prio}),
        reset: () => {
            stopBlink();
            set({
                callDisplay: undefined,
                incomingCalls: [],
            });
        },
    },
}));

const answerCallInCallList = (callId: CallId, targetClientId?: ClientId) =>
    useCallListStore
        .getState()
        .actions.updateCall(callId, {answered: true, clientId: targetClientId});

const rejectCallInCallListIfUnanswered = (callId: CallId) =>
    useCallListStore
        .getState()
        .actions.updateCall(callId, state => ({answered: state.answered || false}));

export const startCall = async (target: CallTarget) => {
    const {cid} = useAuthStore.getState();
    const openErrorOverlay = useErrorOverlayStore.getState().open;

    if (cid === undefined) {
        openErrorOverlay(
            "Unauthenticated",
            "You are unauthenticated and cannot start a call",
            false,
            5000,
        );
        return;
    } else if (target.client === cid) {
        openErrorOverlay("Call error", "You cannot call yourself", false, 5000);
        return;
    }

    const {info} = useConnectionStore.getState();
    const {addOutgoingCall: addOutgoingCallToCallList} = useCallListStore.getState().actions;
    const {prio} = useCallStore.getState();
    const {setOutgoingCall, setPrio} = useCallStore.getState().actions;
    const {defaultSource, temporarySource, setTemporarySource} = useStationsStore.getState();

    let stationId: StationId | undefined;
    if (temporarySource !== undefined) {
        stationId = temporarySource;
        setTemporarySource(undefined);
    } else if (defaultSource !== undefined) {
        stationId = defaultSource;
    }

    const source: CallSource = {
        clientId: cid,
        positionId: info.positionId,
        stationId,
    };

    try {
        const callId = await invokeStrict<CallId>("signaling_start_call", {source, target, prio});
        setOutgoingCall({callId, source, target, prio});
        setPrio(false);
        addOutgoingCallToCallList({callId, target});
    } catch {}
};
