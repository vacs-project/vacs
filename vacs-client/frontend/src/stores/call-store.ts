import {create} from "zustand/react";
import {CallError, invokeStrict} from "../error.ts";
import {useErrorOverlayStore} from "./error-overlay-store.ts";
import {useAuthStore} from "./auth-store.ts";
import {
    Call,
    CallSource,
    CallTarget,
    CallUpdate,
    CallDisplayCall,
    participantCount,
    hasTarget,
} from "../types/call.ts";
import {CallId, ClientId, StationId} from "../types/generic.ts";
import {useConnectionStore} from "./connection-store.ts";
import {useCallListStore} from "./call-list-store.ts";
import {useStationsStore} from "./stations-store.ts";
import {startBlink, tryStopBlink} from "./blink-store.ts";

export type ConnectionState = "connecting" | "connected" | "degraded" | "disconnected";
export type CallDisplayType = "outgoing" | "accepted" | "rejected" | "error";

export type CallDisplay = {
    type: CallDisplayType;
    call: CallDisplayCall;
    erroredTargets: {target: CallTarget; reason: string}[];
    rejectedTargets: CallTarget[];
    errorReason?: string;
};

export type ConferenceState = "modify" | "active" | "inactive";

type CallState = {
    callDisplay?: CallDisplay;
    incomingCalls: Call[];
    prio: boolean;
    conferenceState: ConferenceState;
    actions: {
        setOutgoingCall: (call: CallDisplayCall) => void;
        acceptIncomingCall: (callId: CallId) => void;
        endCall: () => void;
        addIncomingCall: (call: Call) => void;
        updateCall: (update: CallUpdate) => void;
        removeCall: (id: CallId, callEnd?: boolean) => void;
        rejectCall: (id: CallId, targets: CallTarget[]) => void;
        dismissRejectedCall: () => void;
        dismissRejectedTarget: (target: CallTarget) => void;
        errorTargets: (error: CallError) => void;
        dismissErrorCall: () => void;
        dismissErrorTarget: (target: CallTarget) => void;
        setConnectionState: (
            id: CallId,
            peerId: ClientId,
            connectionState: ConnectionState,
        ) => void;
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
                },
            });
        },
        acceptIncomingCall: callId => {
            const incomingCall = get().incomingCalls.find(call => call.callId === callId);
            if (incomingCall === undefined) return;

            const incomingCalls = get().incomingCalls.filter(info => info.callId !== callId);

            tryStopBlink(incomingCalls.length, null, null, null, null);

            answerCallInCallList(callId);

            const callSize =
                incomingCall.invitedTargets.length +
                participantCount(incomingCall.joinedParticipants);

            set({
                callDisplay: {
                    type: "accepted",
                    call: {
                        ...incomingCall,
                        joinedParticipants: Object.assign(
                            {},
                            ...Object.entries(incomingCall.joinedParticipants).map(
                                ([clientId, target]) => ({
                                    [clientId]: {
                                        target,
                                        state: "connecting",
                                    },
                                }),
                            ),
                        ),
                        isConferenceLeader: callSize > 1 ? false : undefined,
                    },
                    rejectedTargets: [],
                    erroredTargets: [],
                },
                incomingCalls,
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
            console.log("received callupdate");
            const incomingCall = get().incomingCalls.find(call => call.callId === update.callId);
            const callDisplay = get().callDisplay;

            if (incomingCall !== undefined) {
                set({
                    incomingCalls: get().incomingCalls.map(call =>
                        call.callId === update.callId ? {...incomingCall, ...update} : call,
                    ),
                });
            } else if (callDisplay?.call.callId === update.callId) {
                const ownClientId = useAuthStore.getState().cid!;
                const type =
                    callDisplay.type === "outgoing"
                        ? Object.keys(update.joinedParticipants).length > 0
                            ? "accepted"
                            : "outgoing"
                        : callDisplay.type;

                const oldJoinedParticipants = callDisplay.call.joinedParticipants;
                const joinedParticipants = Object.entries(update.joinedParticipants).map(
                    ([clientId, target]) => {
                        let oldState = undefined;
                        if (clientId in oldJoinedParticipants) {
                            oldState = oldJoinedParticipants[clientId as ClientId].state;
                        }

                        return {
                            [clientId]: {
                                target,
                                state:
                                    oldState ??
                                    (clientId !== ownClientId ? "connecting" : "connected"),
                            },
                        };
                    },
                );

                const callSize =
                    update.invitedTargets.length + participantCount(update.joinedParticipants);

                let isConferenceLeader = callDisplay.call.isConferenceLeader;
                if (callSize > 2 && callDisplay.call.isConferenceLeader === undefined) {
                    isConferenceLeader = false;
                } else if (callSize <= 2) {
                    isConferenceLeader = undefined;
                    set({conferenceState: "inactive"});
                }

                const targetStillPresent =
                    hasTarget(update.invitedTargets, callDisplay.call.target) ||
                    hasTarget(update.joinedParticipants, callDisplay.call.target);

                const target: CallTarget = targetStillPresent
                    ? callDisplay.call.target
                    : (update.invitedTargets[0] ??
                      Object.values(update.joinedParticipants)[0] ??
                      callDisplay.call.target);

                const nextCallDisplay: CallDisplay = {
                    ...callDisplay,
                    type,
                    call: {
                        ...callDisplay.call,
                        target,
                        invitedTargets: update.invitedTargets,
                        joinedParticipants: Object.assign({}, ...joinedParticipants),
                        isConferenceLeader,
                    },
                };

                set({callDisplay: nextCallDisplay});

                tryStopBlink(null, nextCallDisplay, null, null, null);
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

            const callSize =
                callDisplay.call.invitedTargets.length +
                participantCount(callDisplay.call.joinedParticipants);

            if (callSize <= 2) {
                set({conferenceState: "inactive"});
            }

            if (callSize > 0) {
                callDisplay.rejectedTargets.push(...targets);
            } else {
                callDisplay.type = "rejected";
                callDisplay.rejectedTargets.push(...targets);
            }

            rejectCallInCallListIfUnanswered(callId);

            set({callDisplay});

            startBlink();
        },
        dismissRejectedCall: () => {
            set({callDisplay: undefined});
            tryStopBlink(null, undefined, null, null, null);
        },
        dismissRejectedTarget: target => {
            const callDisplay = get().callDisplay;
            if (callDisplay === undefined) return;

            let nextCallDisplay: CallDisplay = {
                ...callDisplay,
                rejectedTargets: callDisplay.rejectedTargets.filter(
                    rejectedTarget =>
                        rejectedTarget.client !== target.client &&
                        rejectedTarget.position !== target.position &&
                        rejectedTarget.station !== target.station,
                ),
            };

            set({callDisplay: nextCallDisplay});
            tryStopBlink(null, nextCallDisplay, null, null, null);
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

            let targets: CallTarget[] = [];
            if (error.origin.type === "targets") {
                targets = error.origin.value;
            } else if (error.origin.type === "client") {
                targets = [{client: error.origin.value}];
                delete callDisplay.call.joinedParticipants[error.origin.value];
            }

            if (targets.length === 0) {
                callDisplay.call.invitedTargets = [];
                callDisplay.call.joinedParticipants = {};
            } else {
                callDisplay.call.invitedTargets = callDisplay.call.invitedTargets.filter(
                    target =>
                        !targets.some(
                            errorTarget =>
                                errorTarget.client === target.client &&
                                errorTarget.position === target.position &&
                                errorTarget.station === target.station,
                        ),
                );
            }

            const callSize =
                callDisplay.call.invitedTargets.length +
                participantCount(callDisplay.call.joinedParticipants);

            if (callSize <= 2) {
                set({conferenceState: "inactive"});
            }

            if (callSize > 1) {
                callDisplay.erroredTargets.push(
                    ...targets.map(target => ({target, reason: error.reason})),
                );
            } else {
                callDisplay.type = "error";
                callDisplay.erroredTargets.push(
                    ...targets.map(target => ({target, reason: error.reason})),
                );
                callDisplay.errorReason = error.reason;
            }

            set({callDisplay});

            rejectCallInCallListIfUnanswered(callId);

            startBlink();
        },
        dismissErrorCall: () => {
            set({callDisplay: undefined});
            tryStopBlink(null, undefined, null, null, null);
        },
        dismissErrorTarget: target => {
            const callDisplay = get().callDisplay;
            if (callDisplay === undefined) return;

            let nextCallDisplay: CallDisplay = {
                ...callDisplay,
                erroredTargets: callDisplay.erroredTargets.filter(
                    erroredTarget =>
                        erroredTarget.target.client !== target.client &&
                        erroredTarget.target.position !== target.position &&
                        erroredTarget.target.station !== target.station,
                ),
            };

            set({callDisplay: nextCallDisplay});
            tryStopBlink(null, nextCallDisplay, null, null, null);
        },
        setConnectionState: (callId, peerId, connectionState) => {
            let callDisplay = get().callDisplay;

            if (callDisplay === undefined || callDisplay.call.callId !== callId) {
                return;
            }

            callDisplay.call.joinedParticipants[peerId].state = connectionState;

            set({callDisplay});
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

export function someConnectionState(
    callDisplay: CallDisplay | undefined,
    state: ConnectionState,
    excludeSelf?: boolean,
): boolean {
    let joinedParticipants = callDisplay?.call.joinedParticipants;
    if (joinedParticipants === undefined) return false;

    if (excludeSelf === true) {
        const cid = useAuthStore.getState().cid;
        joinedParticipants = {...joinedParticipants};
        delete joinedParticipants[cid as ClientId];
    }

    for (const participant in joinedParticipants) {
        if (joinedParticipants[participant as ClientId].state === state) return true;
    }
    return false;
}

export function allConnectionStates(
    callDisplay: CallDisplay | undefined,
    state: ConnectionState,
): boolean {
    const joinedParticipants = callDisplay?.call.joinedParticipants;
    for (const participant in joinedParticipants) {
        if (joinedParticipants[participant as ClientId].state !== state) return false;
    }
    return true;
}

export const startCall = async (...targets: CallTarget[]) => {
    if (targets.length === 0) return;

    const {callDisplay, conferenceState} = useCallStore.getState();
    const openErrorOverlay = useErrorOverlayStore.getState().open;

    if (callDisplay !== undefined && conferenceState !== "modify") {
        return;
    } else if (callDisplay?.call.isConferenceLeader === false) {
        openErrorOverlay(
            "Call",
            "You are not the conference leader. Can not invite target to call.",
            false,
            5000,
        );
        return;
    }

    const {cid} = useAuthStore.getState();

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
        const callId = await invokeStrict<CallId>("signaling_invite_to_call", {
            source,
            targets,
            prio,
        });

        if (callDisplay !== undefined) {
            useCallStore.setState({
                callDisplay: {
                    ...callDisplay,
                    call: {
                        ...callDisplay.call,
                        invitedTargets: callDisplay.call.invitedTargets.concat(targets),
                        isConferenceLeader: true,
                    },
                },
                conferenceState: "active",
            });
        } else {
            setOutgoingCall({
                callId,
                source,
                target: targets[0],
                invitedTargets: targets,
                joinedParticipants: {},
                isConferenceLeader: targets.length > 1 ? true : undefined,
                prio,
            });
        }

        setPrio(false);
        // TODO: addOutgoingCallToCallList({callId, target});
        return {callId, source, prio};
    } catch {
        return;
    }
};
