import {ConnectionState} from "../stores/call-store.ts";
import {CallId, ClientId, PositionId, StationId} from "./generic.ts";

export type CallSource = {
    clientId: ClientId;
    positionId?: PositionId;
    stationId?: StationId;
};

export type CallTarget = {
    client?: ClientId;
    position?: PositionId;
    station?: StationId;
};

export type CallParticipants = Record<ClientId, CallTarget>;

export type Call = {
    callId: CallId;
    source: CallSource;
    target: CallTarget;
    invitedTargets: CallTarget[];
    joinedParticipants: CallParticipants;
    conferenceLeader?: ClientId | null;
    prio: boolean;
};

export type CallParticipantsWithConnectionState = Record<
    ClientId,
    {target: CallTarget; state: ConnectionState}
>;

export type CallDisplayCall = Omit<Call, "joinedParticipants"> & {
    joinedParticipants: CallParticipantsWithConnectionState;
    isConferenceLeader: boolean | undefined;
    // Targets this client invited itself; the wire carries no per-target
    // inviter. Grow-only, intersect with the live invitedTargets.
    ownInvitedTargets: CallTarget[];
};

export type CallUpdate = {
    callId: CallId;
    invitedTargets: CallTarget[];
    joinedParticipants: CallParticipants;
    conferenceLeader?: ClientId | null;
};

/**
 * Number of parties in the call besides ourselves. `invitedTargets` never
 * contains our own target, and `joinedParticipants` contains us exactly when
 * we joined, so this is phase-independent: use it for every "is this call
 * over" and "is this a conference" decision on a call display.
 */
export function otherPartyCount(call: {
    invitedTargets: CallTarget[];
    joinedParticipants: Record<ClientId, unknown>;
}) {
    return call.invitedTargets.length + participantCount(call.joinedParticipants, true);
}

/**
 * Number of parties in a ringing incoming call besides ourselves. The
 * recipient is in neither list, and the caller only appears in
 * `joinedParticipants` once someone accepts, so a still-ringing caller counts
 * as one extra party.
 */
export function incomingOtherPartyCount(call: {
    source: CallSource;
    invitedTargets: CallTarget[];
    joinedParticipants: Record<ClientId, unknown>;
}) {
    return (
        call.invitedTargets.length +
        participantCount(call.joinedParticipants) +
        (call.source.clientId in call.joinedParticipants ? 0 : 1)
    );
}

export function participantCount(
    participants: Record<ClientId, unknown>,
    excludeSelf: boolean = false,
) {
    return Math.max(Object.keys(participants).length - (excludeSelf ? 1 : 0), 0);
}

export function sameTarget(a: CallTarget, b: CallTarget) {
    return a.client === b.client && a.position === b.position && a.station === b.station;
}

export function hasTarget(
    participants:
        | CallParticipants
        | CallParticipantsWithConnectionState
        | CallTarget[]
        | {target: CallTarget}[],
    target: CallTarget,
) {
    for (const value of Object.values(participants)) {
        if (typeof value.target === "object") {
            if (sameTarget(value.target, target)) {
                return true;
            }
        } else if (sameTarget(value, target)) {
            return true;
        }
    }
    return false;
}

export function callSourceToTarget(source: CallSource): CallTarget {
    if (source.stationId !== undefined) {
        return {station: source.stationId};
    } else if (source.positionId !== undefined) {
        return {position: source.positionId};
    }
    return {client: source.clientId};
}
