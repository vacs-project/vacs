import {useStationsStore} from "../stores/stations-store.ts";
import {
    CallDisplay,
    canAcceptIncomingCall,
    ConferenceState,
    startCall,
    useCallStore,
} from "../stores/call-store.ts";
import {useAsyncDebounce} from "./debounce-hook.ts";
import {invokeSafe, invokeStrict} from "../error.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import {getCallStateColors} from "../utils/call-state-colors.ts";
import {StationId} from "../types/generic.ts";
import {CustomButtonColor} from "../types/custom-button-colors.ts";
import {useBlinkStore} from "../stores/blink-store.ts";
import {CallTarget, hasTarget, participantCount} from "../types/call.ts";

export function keyStationIds(key: {stationId?: StationId; stationIds?: StationId[]}) {
    return key.stationIds ?? (key.stationId !== undefined ? [key.stationId] : []);
}

export function conferenceKeyTargets(
    stationIds: StationId[],
    callDisplay: CallDisplay | undefined,
    stations: Map<StationId, boolean>,
): CallTarget[] {
    return stationIds
        .filter(stationId => stations.get(stationId) === false)
        .map((station): CallTarget => ({station}))
        .filter(
            target =>
                callDisplay === undefined ||
                (!hasTarget(callDisplay.call.joinedParticipants, target) &&
                    !hasTarget(callDisplay.call.invitedTargets, target)),
        );
}

export function shouldInvite(
    targets: CallTarget[],
    callDisplay: CallDisplay | undefined,
    conferenceState: ConferenceState,
): boolean {
    if (targets.length === 0) return false;
    if (callDisplay === undefined) return true;
    return conferenceState === "modify" && callDisplay.call.isConferenceLeader !== false;
}

export function useStationKeyInteraction(
    stationIds: StationId[],
    defaultColor?: CustomButtonColor,
) {
    const blink = useBlinkStore(state => state.blink);
    const stations = useStationsStore(state => state.stations);
    const callDisplay = useCallStore(state => state.callDisplay);
    const conferenceState = useCallStore(state => state.conferenceState);
    const incomingCalls = useCallStore(state => state.incomingCalls);
    const {endCall, cancelInvitedTarget, dismissRejectedTarget, dismissErrorTarget} = useCallStore(
        state => state.actions,
    );

    const defaultStationSource = useStationsStore(state => state.defaultSource);
    const temporaryStationSource = useStationsStore(state => state.temporarySource);
    const setDefaultStationSource = useStationsStore(state => state.setDefaultSource);
    const setTemporaryStationSource = useStationsStore(state => state.setTemporarySource);

    const highlightTarget = useSettingsStore(state => state.callConfig.highlightIncomingCallTarget);
    const enablePrio = useSettingsStore(state => state.callConfig.enablePriorityCalls);
    const endActiveCallOnAccept = useSettingsStore(state => state.callConfig.endActiveCallOnAccept);

    const isConference = stationIds.length > 1;
    const stationId = isConference ? undefined : stationIds[0];
    const hasStationId = stationId !== undefined;
    const station = hasStationId && stations.get(stationId);
    const online = station !== undefined;
    const own = station !== undefined && station;

    const incomingCall = incomingCalls.find(
        call =>
            hasStationId &&
            (call.source.stationId === stationId ||
                hasTarget(call.joinedParticipants, {station: stationId})),
    );
    const isCalling = incomingCall !== undefined && !own;
    const beingCalled =
        hasStationId &&
        !own &&
        callDisplay !== undefined &&
        callDisplay.call.invitedTargets.some(target => target.station === stationId);
    const inCall =
        hasStationId &&
        !own &&
        callDisplay?.type === "accepted" &&
        hasTarget(callDisplay.call.joinedParticipants, {station: stationId});
    const isRejected =
        !own &&
        hasStationId &&
        callDisplay !== undefined &&
        hasTarget(callDisplay.rejectedTargets, {station: stationId});
    const isError =
        !own &&
        hasStationId &&
        callDisplay !== undefined &&
        hasTarget(callDisplay.erroredTargets, {station: stationId});

    const isTarget =
        highlightTarget &&
        hasStationId &&
        (incomingCalls.some(call => call.target.station === stationId) ||
            (own &&
                callDisplay?.type === "accepted" &&
                callDisplay.call.target.station === stationId));

    const handleClick = useAsyncDebounce(async () => {
        if (isConference) {
            const targets = conferenceKeyTargets(stationIds, callDisplay, stations);
            if (shouldInvite(targets, callDisplay, conferenceState)) {
                await startCall(...targets);
            }
            return;
        }
        if (!hasStationId) return;

        if (own) {
            if (defaultStationSource !== stationId && temporaryStationSource !== stationId) {
                setTemporaryStationSource(stationId);
            } else if (
                temporaryStationSource === stationId &&
                defaultStationSource !== stationId &&
                defaultStationSource === undefined
            ) {
                setDefaultStationSource(stationId);
                setTemporaryStationSource(undefined);
            } else if (defaultStationSource === stationId) {
                setDefaultStationSource(undefined);
            } else {
                setTemporaryStationSource(undefined);
            }
            return;
        }

        if (isCalling) {
            if (!canAcceptIncomingCall(callDisplay, endActiveCallOnAccept)) return;

            await invokeSafe("signaling_accept_call", {callId: incomingCall.callId});
        } else if (beingCalled || inCall) {
            const target: CallTarget = {station: stationId};
            const callSize =
                callDisplay.call.invitedTargets.length +
                participantCount(callDisplay.call.joinedParticipants);
            const ownInvited = beingCalled && hasTarget(callDisplay.call.ownInvitedTargets, target);

            if (ownInvited && callSize > 1) {
                // Optimistic; the server's echoed call update converges it.
                try {
                    await invokeStrict("signaling_drop_target", {
                        callId: callDisplay.call.callId,
                        target,
                    });
                    cancelInvitedTarget(callDisplay.call.callId, target);
                } catch {}
            } else if (
                inCall &&
                callDisplay.call.isConferenceLeader &&
                participantCount(callDisplay.call.joinedParticipants) > 2
            ) {
                // Removed once the server confirms the drop via a call update.
                try {
                    await invokeStrict("signaling_drop_target", {
                        callId: callDisplay.call.callId,
                        target,
                    });
                } catch {}
            } else if (beingCalled && !inCall && !ownInvited) {
                // Another participant's pending invitation: display only.
            } else {
                try {
                    await invokeStrict("signaling_end_call", {callId: callDisplay.call.callId});
                    endCall();
                } catch {}
            }
        } else if (isRejected) {
            dismissRejectedTarget({station: stationId});
        } else if (isError) {
            dismissErrorTarget({station: stationId});
        } else {
            await startCall({station: stationId});
        }
    });

    const prio =
        enablePrio &&
        (hasTarget(callDisplay?.prioTargets ?? [], {station: stationId}) ||
            (incomingCall?.prio ?? false));

    const {color, highlight} = getCallStateColors({
        inCall,
        isCalling,
        beingCalled,
        isRejected,
        isError,
        isTarget,
        prio,
        blink,
        temporarySource:
            temporaryStationSource === stationId && temporaryStationSource !== undefined,
        defaultSource: defaultStationSource === stationId && defaultStationSource !== undefined,
        defaultColor,
    });

    const disabled = isConference
        ? !stationIds.some(id => stations.get(id) === false)
        : !hasStationId || !online;

    return {color, highlight, disabled, own, handleClick};
}
