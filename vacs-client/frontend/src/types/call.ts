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

export type CallUpdate = {
    callId: CallId;
    invitedTargets: CallTarget[];
    joinedParticipants: CallParticipants;
};

export function participantCount(participants: CallParticipants, excludeSelf: boolean = false) {
    return Math.max(Object.keys(participants).length - (excludeSelf ? 1 : 0), 0);
}

export function hasTarget(participants: CallParticipants, target: CallTarget) {
    for (const value of Object.values(participants)) {
        if (
            value.client === target.client &&
            value.position === target.position &&
            value.station === target.station
        )
            return true;
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
