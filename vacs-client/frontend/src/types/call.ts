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
};

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
