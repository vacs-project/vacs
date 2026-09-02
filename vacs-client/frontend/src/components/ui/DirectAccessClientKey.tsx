import {ClientInfo, ClientPageConfig, splitDisplayName} from "../../types/client.ts";
import Button from "./Button.tsx";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {invokeSafe, invokeStrict} from "../../error.ts";
import {startCall, useCallStore} from "../../stores/call-store.ts";
import {clsx} from "clsx";
import {useSettingsStore} from "../../stores/settings-store.ts";
import {getCallStateColors} from "../../utils/call-state-colors.ts";
import {useBlinkStore} from "../../stores/blink-store.ts";
import {CallTarget, hasTarget, participantCount} from "../../types/call.ts";

type DAKeyProps = {
    client: ClientInfo;
    config: ClientPageConfig | undefined;
};

function DirectAccessClientKey({client, config}: DAKeyProps) {
    const blink = useBlinkStore(state => state.blink);
    const callDisplay = useCallStore(state => state.callDisplay);
    const incomingCalls = useCallStore(state => state.incomingCalls);
    const {endCall, cancelInvitedTarget, dismissRejectedTarget, dismissErrorTarget} = useCallStore(
        state => state.actions,
    );
    const enablePrio = useSettingsStore(state => state.callConfig.enablePriorityCalls);

    const incomingCall = incomingCalls.find(
        call =>
            call.source.clientId === client.id ||
            Object.keys(call.joinedParticipants).includes(client.id),
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

            await invokeSafe("signaling_accept_call", {callId: incomingCall.callId});
        } else if (beingCalled || inCall) {
            const target: CallTarget = {client: client.id};
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
        (callDisplay !== undefined
            ? hasTarget(callDisplay.prioTargets, {client: client.id})
            : (incomingCall?.prio ?? false));

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
