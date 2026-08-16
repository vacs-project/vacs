import {useCallStore} from "../stores/call-store.ts";
import {useAuthStore} from "../stores/auth-store.ts";
import {DirectAccessPage} from "../types/profile.ts";
import {CallDisplayCall, hasTarget} from "../types/call.ts";
import {ClientId, StationId} from "../types/generic.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import {getCallStateColors} from "../utils/call-state-colors.ts";
import {CustomButtonColor} from "../types/custom-button-colors.ts";
import {useBlinkStore} from "../stores/blink-store.ts";

export function useCallState(page: DirectAccessPage | undefined, defaultColor?: CustomButtonColor) {
    const blink = useBlinkStore(state => state.blink);
    const callDisplay = useCallStore(state => state.callDisplay);
    const incomingCalls = useCallStore(state => state.incomingCalls);
    const cid = useAuthStore(state => state.cid);

    const highlightTarget = useSettingsStore(state => state.callConfig.highlightIncomingCallTarget);
    const enablePrio = useSettingsStore(state => state.callConfig.enablePriorityCalls);

    const stationIds = directAccessPageToStationIds(page);

    const incomingCall = incomingCalls.find(
        call =>
            call.source.stationId !== undefined &&
            (stationIds.includes(call.source.stationId) ||
                stationIds.some(stationId =>
                    hasTarget(call.joinedParticipants, {station: stationId}),
                )),
    );
    const isCalling = incomingCall !== undefined;
    const beingCalled = stationIds.some(stationId =>
        callDisplay?.call.invitedTargets.some(target => target.station === stationId),
    );

    const inCall =
        callDisplay?.type === "accepted" &&
        callDisplay !== undefined &&
        inCallWithButtonStations(callDisplay.call, stationIds, cid);
    const isRejected =
        callDisplay !== undefined &&
        stationIds.some(stationId => hasTarget(callDisplay.rejectedTargets, {station: stationId}));
    const isError =
        callDisplay !== undefined &&
        stationIds.some(stationId => hasTarget(callDisplay.erroredTargets, {station: stationId}));
    const isTarget =
        highlightTarget &&
        (incomingCalls.some(
            call => call.target.station !== undefined && stationIds.includes(call.target.station),
        ) ||
            (callDisplay?.type === "accepted" &&
                callDisplay.call.target.station !== undefined &&
                stationIds.includes(callDisplay.call.target.station)));

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
        defaultColor,
    });

    return {isCalling, beingCalled, inCall, isRejected, isError, isTarget, color, highlight, blink};
}

function inCallWithButtonStations(
    call: CallDisplayCall,
    stationIds: StationId[],
    cid: ClientId | undefined,
) {
    return stationIds.some(stationId =>
        hasTarget(
            Object.entries(call.joinedParticipants).flatMap(([clientId, value]) =>
                clientId !== cid ? [value.target] : [],
            ),
            {station: stationId},
        ),
    );
}

export function directAccessPageToStationIds(page: DirectAccessPage | undefined): StationId[] {
    const result: StationId[] = [];

    function visit(page: DirectAccessPage | undefined) {
        if (page === undefined || page.keys === undefined) return;

        for (const key of page.keys) {
            if (key.stationId !== undefined) result.push(key.stationId);
            visit(key.page);
        }
    }

    visit(page);

    return result;
}
