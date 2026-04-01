import {useStationsStore} from "../stores/stations-store.ts";
import {startCall, useCallStore} from "../stores/call-store.ts";
import {useAsyncDebounce} from "./debounce-hook.ts";
import {invokeSafe, invokeStrict} from "../error.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import {getCallStateColors} from "../utils/call-state-colors.ts";
import {StationId} from "../types/generic.ts";
import {CustomButtonColor} from "../types/custom-button-colors.ts";

export function useStationKeyInteraction(
    stationId: StationId | undefined,
    defaultColor?: CustomButtonColor,
) {
    const blink = useCallStore(state => state.blink);
    const stations = useStationsStore(state => state.stations);
    const callDisplay = useCallStore(state => state.callDisplay);
    const incomingCalls = useCallStore(state => state.incomingCalls);
    const {endCall, dismissRejectedCall, dismissErrorCall} = useCallStore(state => state.actions);

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
        call => hasStationId && call.source.stationId === stationId,
    );
    const isCalling = incomingCall !== undefined && !own;
    const beingCalled =
        hasStationId &&
        !own &&
        callDisplay?.type === "outgoing" &&
        callDisplay.call.target.station === stationId;
    const involved =
        !own &&
        callDisplay !== undefined &&
        (callDisplay.call.source.stationId === stationId ||
            callDisplay.call.target.station === stationId);
    const inCall = hasStationId && involved && callDisplay.type === "accepted";
    const isRejected = hasStationId && involved && callDisplay?.type === "rejected";
    const isError = hasStationId && involved && callDisplay?.type === "error";

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
            try {
                await invokeStrict("signaling_end_call", {callId: callDisplay.call.callId});
                endCall();
            } catch {}
        } else if (isRejected) {
            dismissRejectedCall();
        } else if (isError) {
            dismissErrorCall();
        } else if (callDisplay === undefined) {
            await startCall({station: stationId});
        }
    });

    const outgoingPrio = callDisplay?.call.prio === true && enablePrio;
    const incomingPrio = incomingCall?.prio === true && enablePrio;

    const {color, highlight} = getCallStateColors({
        inCall,
        isCalling,
        beingCalled,
        isRejected,
        isError,
        isTarget,
        outgoingPrio,
        incomingPrio,
        blink,
        temporarySource:
            temporaryStationSource === stationId && temporaryStationSource !== undefined,
        defaultSource: defaultStationSource === stationId && defaultStationSource !== undefined,
        defaultColor,
    });

    return {color, highlight, disabled: !hasStationId || !online, own, handleClick};
}
