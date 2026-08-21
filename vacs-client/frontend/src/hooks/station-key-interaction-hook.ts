import {useStationsStore} from "../stores/stations-store.ts";
import {startCall, useCallStore} from "../stores/call-store.ts";
import {useAsyncDebounce} from "./debounce-hook.ts";
import {invokeSafe, invokeStrict} from "../error.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import {getCallStateColors} from "../utils/call-state-colors.ts";
import {StationId} from "../types/generic.ts";
import {CustomButtonColor} from "../types/custom-button-colors.ts";
import {useBlinkStore} from "../stores/blink-store.ts";
import {CallTarget, hasTarget, participantCount} from "../types/call.ts";

export function useStationKeyInteraction(
    stationId: StationId | undefined,
    defaultColor?: CustomButtonColor,
) {
    const blink = useBlinkStore(state => state.blink);
    const stations = useStationsStore(state => state.stations);
    const callDisplay = useCallStore(state => state.callDisplay);
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
            if (callDisplay !== undefined) return;
            await invokeSafe("signaling_accept_call", {callId: incomingCall.callId});
        } else if (beingCalled || inCall) {
            const target: CallTarget = {station: stationId};
            const callSize =
                callDisplay.call.invitedTargets.length +
                participantCount(callDisplay.call.joinedParticipants);
            const ownInvited = beingCalled && hasTarget(callDisplay.call.ownInvitedTargets, target);

            if (ownInvited && callSize > 1) {
                // Optimistic: a caller without joined participants gets no
                // confirming call update.
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
        (callDisplay !== undefined
            ? hasTarget(callDisplay.prioTargets, {station: stationId})
            : (incomingCall?.prio ?? false));

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

    return {color, highlight, disabled: !hasStationId || !online, own, handleClick};
}
