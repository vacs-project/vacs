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
    callSourceToTarget,
} from "../types/call.ts";
import {CallId, ClientId, StationId} from "../types/generic.ts";
import {useConnectionStore} from "./connection-store.ts";
import {CallListTarget, useCallListStore} from "./call-list-store.ts";
import {useStationsStore} from "./stations-store.ts";
import {startBlink, tryStopBlink} from "./blink-store.ts";

export type ConnectionState = "connecting" | "connected" | "degraded" | "disconnected";
export type CallDisplayType = "outgoing" | "accepted" | "rejected" | "error";

export type CallDisplay = {
    type: CallDisplayType;
    call: CallDisplayCall;
    prioTargets: CallTarget[];
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
        cancelInvitedTarget: (id: CallId, target: CallTarget) => void;
        rejectTargets: (id: CallId, targets: CallTarget[]) => void;
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
                    prioTargets: call.prio ? call.invitedTargets : [],
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

            updateCallListEntry(callId, true, undefined);

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
                        isConferenceLeader: deriveIsConferenceLeader(
                            incomingCall.conferenceLeader,
                            useAuthStore.getState().cid,
                        ),
                        ownInvitedTargets: [],
                    },
                    prioTargets: incomingCall.prio ? [callSourceToTarget(incomingCall.source)] : [],
                    rejectedTargets: [],
                    erroredTargets: [],
                },
                incomingCalls,
            });
        },
        endCall: () => {
            tryStopBlink(null, undefined, null, null, "inactive");
            set({callDisplay: undefined, conferenceState: "inactive"});
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

                updateCallListEntry(update.callId, undefined, callListTargets(update));
            } else if (callDisplay?.call.callId === update.callId) {
                // A terminal display stays terminal: the call is already over
                // for this client and only a dismiss clears it. Applying the
                // update would resurrect it as a live-looking call.
                if (callDisplay.type === "error" || callDisplay.type === "rejected") {
                    return;
                }

                const ownClientId = useAuthStore.getState().cid!;

                const isAccepted =
                    callDisplay.type === "outgoing" &&
                    Object.keys(update.joinedParticipants).length > 0;
                const type = isAccepted ? "accepted" : callDisplay.type;

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

                const isConferenceLeader = deriveIsConferenceLeader(
                    update.conferenceLeader,
                    ownClientId,
                );
                if (callSize <= 2) {
                    set({conferenceState: "inactive"});
                }

                const targetStillPresent =
                    hasTarget(update.invitedTargets, callDisplay.call.target) ||
                    hasTarget(update.joinedParticipants, callDisplay.call.target);

                const target: CallTarget = targetStillPresent
                    ? callDisplay.call.target
                    : (update.invitedTargets[0] ??
                      Object.entries(update.joinedParticipants).flatMap(([clientId, target]) =>
                          clientId !== ownClientId ? [target] : [],
                      )[0] ??
                      callDisplay.call.target);

                const sourceAsTarget = callSourceToTarget(callDisplay.call.source);
                const sourceStillPresent =
                    hasTarget(update.invitedTargets, sourceAsTarget) ||
                    hasTarget(update.joinedParticipants, sourceAsTarget);

                const source: CallSource = sourceStillPresent
                    ? callDisplay.call.source
                    : (Object.entries(update.joinedParticipants).flatMap(([clientId, target]) =>
                          clientId !== ownClientId
                              ? [
                                    {
                                        clientId: clientId as ClientId,
                                        positionId: target.position,
                                        stationId: target.station,
                                    },
                                ]
                              : [],
                      )[0] ?? callDisplay.call.source);

                const nextCallDisplay: CallDisplay = {
                    ...callDisplay,
                    type,
                    call: {
                        ...callDisplay.call,
                        source,
                        target,
                        invitedTargets: update.invitedTargets,
                        joinedParticipants: Object.assign({}, ...joinedParticipants),
                        isConferenceLeader,
                    },
                    prioTargets: callDisplay.prioTargets.filter(
                        target =>
                            hasTarget(update.invitedTargets, target) ||
                            hasTarget(update.joinedParticipants, target),
                    ),
                    // A re-invited target is no longer rejected or errored.
                    rejectedTargets: callDisplay.rejectedTargets.filter(
                        target => !hasTarget(update.invitedTargets, target),
                    ),
                    erroredTargets: callDisplay.erroredTargets.filter(
                        errored => !hasTarget(update.invitedTargets, errored.target),
                    ),
                };

                set({callDisplay: nextCallDisplay});

                updateCallListEntry(
                    update.callId,
                    isAccepted ? true : undefined,
                    callListTargets(update),
                );

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

            rejectCallListEntryIfUnanswered(callId);

            tryStopBlink(incomingCalls.length, callDisplay, null, null, conferenceState);
            set({incomingCalls, callDisplay, conferenceState});
        },
        cancelInvitedTarget: (callId, target) => {
            const callDisplay = get().callDisplay;
            if (callDisplay === undefined || callDisplay.call.callId !== callId) return;

            const invitedTargets = callDisplay.call.invitedTargets.filter(
                invited => !hasTarget([target], invited),
            );

            const nextCallDisplay: CallDisplay = {
                ...callDisplay,
                call: {...callDisplay.call, invitedTargets},
                prioTargets: callDisplay.prioTargets.filter(
                    prioTarget =>
                        hasTarget(invitedTargets, prioTarget) ||
                        hasTarget(callDisplay.call.joinedParticipants, prioTarget),
                ),
            };

            const callSize =
                invitedTargets.length + participantCount(callDisplay.call.joinedParticipants);
            const conferenceState = callSize <= 2 ? "inactive" : get().conferenceState;

            set({callDisplay: nextCallDisplay, conferenceState});
            tryStopBlink(null, nextCallDisplay, null, null, conferenceState);
        },
        rejectTargets: (callId, targets) => {
            let callDisplay = get().callDisplay;

            if (callDisplay === undefined || callDisplay.call.callId !== callId) {
                get().actions.removeCall(callId);
                return;
            }

            callDisplay = structuredClone(callDisplay);

            targets = targets.filter(target => hasTarget(callDisplay.call.invitedTargets, target));

            callDisplay.call.invitedTargets = callDisplay.call.invitedTargets.filter(
                target => !hasTarget(targets, target),
            );

            callDisplay.prioTargets = callDisplay.prioTargets.filter(
                target =>
                    hasTarget(callDisplay.call.invitedTargets, target) ||
                    hasTarget(callDisplay.call.joinedParticipants, target),
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

                rejectCallListEntryIfUnanswered(callId);
            }

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
                        !(
                            rejectedTarget.client === target.client &&
                            rejectedTarget.position === target.position &&
                            rejectedTarget.station === target.station
                        ),
                ),
            };

            set({callDisplay: nextCallDisplay});
            tryStopBlink(null, nextCallDisplay, null, null, null);
        },
        errorTargets: error => {
            const callId = error.callId;
            let callDisplay = get().callDisplay;

            if (callDisplay === undefined || callDisplay.call.callId !== callId) {
                get().actions.removeCall(callId);
                return;
            }

            callDisplay = structuredClone(callDisplay);

            let targets: CallTarget[];
            if (error.origin.type === "call") {
                targets = callDisplay.call.invitedTargets;
                callDisplay.call.invitedTargets = [];

                targets.push(
                    ...Object.values(callDisplay.call.joinedParticipants).map(
                        value => value.target,
                    ),
                );
                callDisplay.call.joinedParticipants = {};

                callDisplay.prioTargets = [];
            } else {
                if (error.origin.type === "targets") {
                    targets = error.origin.value.filter(
                        target =>
                            hasTarget(callDisplay.call.invitedTargets, target) ||
                            hasTarget(callDisplay.call.joinedParticipants, target),
                    );

                    if (targets.length === 0) return;
                } else if (error.origin.type === "client") {
                    const joinedParticipant =
                        callDisplay.call.joinedParticipants[error.origin.value];

                    if (joinedParticipant === undefined) return;
                    targets = [joinedParticipant.target];
                    delete callDisplay.call.joinedParticipants[error.origin.value];
                } else {
                    return;
                }

                callDisplay.call.invitedTargets = callDisplay.call.invitedTargets.filter(
                    target => !hasTarget(targets, target),
                );
                callDisplay.prioTargets = callDisplay.prioTargets.filter(
                    target =>
                        hasTarget(callDisplay.call.invitedTargets, target) ||
                        hasTarget(callDisplay.call.joinedParticipants, target),
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

                rejectCallListEntryIfUnanswered(callId);
            }

            set({callDisplay});

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
                        !(
                            erroredTarget.target.client === target.client &&
                            erroredTarget.target.position === target.position &&
                            erroredTarget.target.station === target.station
                        ),
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

            callDisplay = structuredClone(callDisplay);

            const joinedParticipant = callDisplay.call.joinedParticipants[peerId];

            if (joinedParticipant === undefined) return;

            joinedParticipant.state = connectionState;

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

const deriveIsConferenceLeader = (
    conferenceLeader: ClientId | null | undefined,
    ownClientId: ClientId | undefined,
): boolean | undefined => (conferenceLeader == null ? undefined : conferenceLeader === ownClientId);

const updateCallListEntry = (
    callId: CallId,
    answered: boolean | undefined,
    targets: CallListTarget[] | undefined,
) => useCallListStore.getState().actions.updateCallListEntry(callId, {answered, targets});

/**
 * Extends an existing entry by newly invited targets. A conference-add reuses the call id of
 * the current call, so its entry must be extended instead of replaced by a fresh one.
 */
const addTargetsToCallListEntry = (callId: CallId, targets: CallTarget[]) =>
    useCallListStore.getState().actions.updateCallListEntry(callId, entry => ({
        targets: entry.targets.concat(
            targets
                .filter(target => !hasTarget(entry.targets, target))
                .map(target => ({target, clientId: target.client})),
        ),
    }));

const rejectCallListEntryIfUnanswered = (callId: CallId) =>
    useCallListStore
        .getState()
        .actions.updateCallListEntry(callId, state => ({answered: state.answered || false}));

const callListTargets = (update: CallUpdate): CallListTarget[] => {
    const ownClientId = useAuthStore.getState().cid;
    const targets: CallListTarget[] = [];

    for (const [clientId, target] of Object.entries(update.joinedParticipants)) {
        if (clientId === ownClientId) continue;
        targets.push({target, clientId: clientId as ClientId});
    }

    // invitedTargets never contains the recipient's own target.
    for (const target of update.invitedTargets) {
        if (hasTarget(update.joinedParticipants, target)) continue;
        targets.push({target, clientId: target.client});
    }

    return targets;
};

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
    const {addOutgoingCallListEntry} = useCallListStore.getState().actions;
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
        if (callDisplay !== undefined) {
            useCallStore.setState({
                callDisplay: {
                    ...callDisplay,
                    call: {
                        ...callDisplay.call,
                        invitedTargets: callDisplay.call.invitedTargets.concat(targets),
                        ownInvitedTargets: callDisplay.call.ownInvitedTargets.concat(targets),
                        isConferenceLeader: true,
                    },
                    prioTargets: prio
                        ? callDisplay.prioTargets.concat(targets)
                        : callDisplay.prioTargets,
                    // A re-invited target is no longer rejected or errored.
                    rejectedTargets: callDisplay.rejectedTargets.filter(
                        target => !hasTarget(targets, target),
                    ),
                    erroredTargets: callDisplay.erroredTargets.filter(
                        errored => !hasTarget(targets, errored.target),
                    ),
                },
                conferenceState: "active",
            });
            addTargetsToCallListEntry(callDisplay.call.callId, targets);
        }

        const callId = await invokeStrict<CallId>("signaling_invite_to_call", {
            source,
            targets,
            prio,
        });

        if (callDisplay === undefined) {
            setOutgoingCall({
                callId,
                source,
                target: targets[0],
                invitedTargets: targets,
                ownInvitedTargets: targets,
                joinedParticipants: {},
                isConferenceLeader: targets.length > 1 ? true : undefined,
                prio,
            });
            addOutgoingCallListEntry({callId, targets});
        }

        setPrio(false);
        return {callId, source, prio};
    } catch {
        return;
    }
};
