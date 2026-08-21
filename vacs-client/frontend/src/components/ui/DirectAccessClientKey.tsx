import {ClientInfo, ClientPageConfig, splitDisplayName} from "../../types/client.ts";
import Button from "./Button.tsx";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {invokeStrict} from "../../error.ts";
import {startCall, useCallStore} from "../../stores/call-store.ts";
import {clsx} from "clsx";
import {useSettingsStore} from "../../stores/settings-store.ts";
import {getCallStateColors} from "../../utils/call-state-colors.ts";
import {useBlinkStore} from "../../stores/blink-store.ts";
import {hasTarget, participantCount} from "../../types/call.ts";

type DAKeyProps = {
    client: ClientInfo;
    config: ClientPageConfig | undefined;
};

function DirectAccessClientKey({client, config}: DAKeyProps) {
    const blink = useBlinkStore(state => state.blink);
    const callDisplay = useCallStore(state => state.callDisplay);
    const incomingCalls = useCallStore(state => state.incomingCalls);
    const {endCall, updateCall, dismissRejectedTarget, dismissErrorTarget} = useCallStore(
        state => state.actions,
    );
    const enablePrio = useSettingsStore(state => state.callConfig.enablePriorityCalls);

    const incomingCall = incomingCalls.find(
        call =>
            call.source.clientId === client.id ||
            hasTarget(call.joinedParticipants, {client: client.id}),
    );
    const isCalling = incomingCall !== undefined;
    const beingCalled =
        callDisplay !== undefined &&
        callDisplay.call.invitedTargets.some(target => target.client === client.id);
    const inCall =
        callDisplay?.type === "accepted" &&
        Object.keys(callDisplay.call.joinedParticipants).includes(client.id);
    const isRejected =
        callDisplay !== undefined && hasTarget(callDisplay.rejectedTargets, {client: client.id});
    const isError =
        callDisplay !== undefined && hasTarget(callDisplay.erroredTargets, {client: client.id});

    const handleClick = useAsyncDebounce(async () => {
        if (isCalling) {
            if (callDisplay !== undefined) return;

            try {
                await invokeStrict("signaling_accept_call", {callId: incomingCall.callId});
            } catch {}
        } else if (beingCalled || inCall) {
            if (
                callDisplay.call.invitedTargets.length +
                    participantCount(callDisplay.call.joinedParticipants) >
                    2 &&
                callDisplay.call.isConferenceLeader
            ) {
                try {
                    await invokeStrict("signaling_drop_target", {
                        callId: callDisplay.call.callId,
                        target: {client: client.id},
                    });
                    updateCall({
                        callId: callDisplay.call.callId,
                        invitedTargets: callDisplay.call.invitedTargets.filter(
                            target =>
                                target.client !== client.id &&
                                target.position === undefined &&
                                target.station === undefined,
                        ),
                        joinedParticipants: Object.assign(
                            {},
                            ...Object.entries(callDisplay.call.joinedParticipants)
                                .filter(
                                    ([_, value]) =>
                                        value.target.client !== client.id &&
                                        value.target.position === undefined &&
                                        value.target.station === undefined,
                                )
                                .map(([clientId, value]) => ({
                                    [clientId]: value,
                                })),
                        ),
                    });
                } catch {}
            } else {
                try {
                    await invokeStrict("signaling_end_call", {callId: callDisplay.call.callId});
                    endCall();
                } catch {}
            }
        } else if (isRejected) {
            dismissRejectedTarget({client: client.id});
        } else if (isError) {
            dismissErrorTarget({client: client.id});
        } else {
            await startCall({client: client.id});
        }
    });

    const [stationName, stationType] = splitDisplayName(client.displayName);
    const showFrequency = client.frequency !== "" && config?.frequencies === "ShowAll";

    const prio =
        enablePrio &&
        callDisplay !== undefined &&
        hasTarget(callDisplay.prioTargets, {client: client.id});

    const {color, highlight} = getCallStateColors({
        inCall,
        isCalling,
        beingCalled,
        isRejected,
        isError,
        prio,
        blink,
    });

    return (
        <Button
            color={color}
            className={clsx(
                "w-25 h-full rounded leading-4.5!",
                color === "gray" ? "p-1.5" : "p-[calc(0.375rem+1px)]",
            )}
            highlight={highlight}
            onClick={handleClick}
        >
            <p className="w-full truncate" title={client.displayName}>
                {stationName}
            </p>
            {stationType !== "" && <p>{stationType}</p>}
            {showFrequency && <p title={client.frequency}>{client.frequency}</p>}
        </Button>
    );
}

export default DirectAccessClientKey;
