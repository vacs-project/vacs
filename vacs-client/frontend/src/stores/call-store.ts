import {create} from "zustand/react";
import {CallError, invokeStrict} from "../error.ts";
import {useErrorOverlayStore} from "./error-overlay-store.ts";
import {useAuthStore} from "./auth-store.ts";
import {Call, CallSource, CallTarget, CallUpdate} from "../types/call.ts";
import {CallId, ClientId, StationId} from "../types/generic.ts";
import {useConnectionStore} from "./connection-store.ts";
import {useCallListStore} from "./call-list-store.ts";
import {useStationsStore} from "./stations-store.ts";
import {startBlink, tryStopBlink} from "./blink-store.ts";

export type ConnectionState = "connecting" | "connected" | "degraded" | "disconnected";
export type CallDisplayType = "outgoing" | "accepted" | "rejected" | "error";

export type CallDisplay = {
    type: CallDisplayType;
    call: Call;
    targetClientId?: ClientId;
    erroredTargets: {target: CallTarget; reason: string}[];
    rejectedTargets: CallTarget[];
    errorReason?: string;
    connectionState?: ConnectionState;
};

export type ConferenceState = "modify" | "active" | "inactive";

type CallState = {
    callDisplay?: CallDisplay;
    incomingCalls: Call[];
    prio: boolean;
    conferenceState: ConferenceState;
    actions: {
        setOutgoingCall: (call: Call) => void;
        acceptIncomingCall: (callId: CallId) => void;
        setOutgoingCallAccepted: (calLId: CallId, targetClientId: ClientId) => void;
        endCall: () => void;
        addIncomingCall: (call: Call) => void;
        updateCall: (update: CallUpdate) => void;
        removeCall: (id: CallId, callEnd?: boolean) => void;
        rejectCall: (id: CallId, targets: CallTarget[]) => void;
        dismissRejectedCall: () => void;
        errorTargets: (error: CallError) => void;
        dismissErrorCall: () => void;
        setConnectionState: (id: CallId, connectionState: ConnectionState) => void;
        setPrio: (prio: boolean) => void;
        setConferenceState: (conferenceState: ConferenceState) => void;
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
    conferenceState: "inactive",
    actions: {
        setOutgoingCall: call => {
            if (call.prio) {
                startBlink();
            }

            set({
                callDisplay: {
                    type: "outgoing",
                    call,
                    rejectedTargets: [],
                    erroredTargets: [],
                    connectionState: undefined,
                },
            });
        },
        acceptIncomingCall: callId => {
            const incomingCall = get().incomingCalls.find(call => call.callId === callId);
            if (incomingCall === undefined) return;

            const incomingCalls = get().incomingCalls.filter(info => info.callId !== callId);

            tryStopBlink(incomingCalls.length, null, null, null, null);

            answerCallInCallList(callId);

            set({
                callDisplay: {
                    type: "accepted",
                    call: incomingCall,
                    targetClientId: incomingCall.source.clientId,
                    rejectedTargets: [],
                    erroredTargets: [],
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
            tryStopBlink(null, nextCallDisplay, null, null, null);

            answerCallInCallList(callId, targetClientId);

            set({
                callDisplay: nextCallDisplay,
            });
        },
        endCall: () => {
            tryStopBlink(null, undefined, null, null, "inactive");
            set({callDisplay: undefined, conferenceState: "inactive"}); // TODO: conference state validate
        },
        addIncomingCall: call => {
            const incomingCalls = get().incomingCalls.filter(info => info.callId !== call.callId);

            startBlink();

            set({incomingCalls: [...incomingCalls, call]});
        },
        updateCall: update => {
            const incomingCall = get().incomingCalls.find(call => call.callId === update.callId);
            const callDisplay = get().callDisplay;

            if (incomingCall !== undefined) {
                set({
                    incomingCalls: get().incomingCalls.map(call =>
                        call.callId === update.callId ? {...incomingCall, ...update} : call,
                    ),
                });
            } else if (callDisplay?.call.callId === update.callId) {
                set({
                    callDisplay: {
                        ...callDisplay,
                        call: {...callDisplay.call, ...update},
                    },
                });
            }
        },
        removeCall: (callId, callEnd) => {
            const incomingCalls = get().incomingCalls.filter(info => info.callId !== callId);
            let callDisplay = get().callDisplay;
            let conferenceState = get().conferenceState;

            if (
                callDisplay?.call.callId === callId &&
                callDisplay?.type !== "error" &&
                (!callEnd || callDisplay?.type !== "outgoing")
            ) {
                callDisplay = undefined;
                conferenceState = "inactive";
            }

            rejectCallInCallListIfUnanswered(callId);

            tryStopBlink(incomingCalls.length, callDisplay, null, null, conferenceState); // TODO: validate conference state
            set({incomingCalls, callDisplay, conferenceState});
        },
        rejectCall: (callId, targets) => {
            let callDisplay = get().callDisplay;

            if (
                callDisplay === undefined ||
                callDisplay.call.callId !== callId ||
                callDisplay.type !== "outgoing"
            ) {
                get().actions.removeCall(callId);
                return;
            }

            callDisplay.call.invitedTargets = callDisplay.call.invitedTargets.filter(
                target =>
                    !targets.some(
                        rejectedTarget =>
                            rejectedTarget.client === target.client &&
                            rejectedTarget.position === target.position &&
                            rejectedTarget.station === target.station,
                    ),
            );

            if (
                callDisplay.call.invitedTargets.length + callDisplay.call.joinedParticipants.size >
                0
            ) {
                callDisplay.rejectedTargets.push(...targets);
            } else {
                callDisplay.type = "rejected";
                callDisplay.rejectedTargets.push(...targets);
                callDisplay.connectionState = undefined;
            }

            rejectCallInCallListIfUnanswered(callId);

            set({callDisplay});

            startBlink();
        },
        dismissRejectedCall: () => {
            set({callDisplay: undefined});
            tryStopBlink(null, undefined, null, null, null);
        },
        errorTargets: error => {
            const callId = error.callId;
            let callDisplay = get().callDisplay;

            if (
                callDisplay === undefined ||
                callDisplay.call.callId !== callId ||
                callDisplay.type === "rejected"
            ) {
                get().actions.removeCall(callId);
                return;
            }

            if (error.targets.length === 0) {
                callDisplay.call.invitedTargets = [];
                callDisplay.call.joinedParticipants = new Map();
            } else {
                callDisplay.call.invitedTargets = callDisplay.call.invitedTargets.filter(
                    target =>
                        !error.targets.some(
                            errorTarget =>
                                errorTarget.client === target.client &&
                                errorTarget.position === target.position &&
                                errorTarget.station === target.station,
                        ),
                );
            }

            if (
                callDisplay.call.invitedTargets.length + callDisplay.call.joinedParticipants.size >
                0
            ) {
                callDisplay.erroredTargets.push(
                    ...error.targets.map(target => ({target, reason: error.reason})),
                );
            } else {
                callDisplay.type = "error";
                callDisplay.erroredTargets.push(
                    ...error.targets.map(target => ({target, reason: error.reason})),
                );
                callDisplay.errorReason = error.reason;
                callDisplay.connectionState = undefined;
            }

            set({callDisplay});

            rejectCallInCallListIfUnanswered(callId);

            startBlink();
        },
        dismissErrorCall: () => {
            set({callDisplay: undefined});
            tryStopBlink(null, undefined, null, null, null);
        },
        setConnectionState: (callId, connectionState) => {
            const callDisplay = get().callDisplay;

            if (callDisplay === undefined || callDisplay.call.callId !== callId) {
                return;
            }

            set({callDisplay: {...callDisplay, connectionState}});
        },
        setPrio: prio => set({prio}),
        setConferenceState: conferenceState => {
            if (conferenceState === "modify") {
                startBlink();
            } else {
                tryStopBlink(null, null, null, null, "inactive");
            }

            set({conferenceState});
        },
        reset: () => {
            tryStopBlink(0, undefined, null, null, "inactive");
            set({
                callDisplay: undefined,
                incomingCalls: [],
                conferenceState: "inactive",
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

export const startCall = async (...targets: CallTarget[]) => {
    if (targets.length === 0) return;

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
    } else if (targets.some(target => target.client === cid)) {
        openErrorOverlay("Call error", "You cannot call yourself", false, 5000);
        return;
    }

    const {info} = useConnectionStore.getState();
    // const {addOutgoingCall: addOutgoingCallToCallList} = useCallListStore.getState().actions;
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
        const callId = await invokeStrict<CallId>("signaling_start_call", {source, targets, prio});
        setOutgoingCall({
            callId,
            source,
            target: targets[0],
            invitedTargets: targets,
            joinedParticipants: new Map(),
            prio,
        });
        setPrio(false);
        // TODO: addOutgoingCallToCallList({callId, target});
    } catch {}
};
