import {ClientInfo, ClientPageConfig, splitDisplayName} from "../../types/client.ts";
import Button from "./Button.tsx";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {invokeStrict} from "../../error.ts";
import {startCall, useCallStore} from "../../stores/call-store.ts";
import {clsx} from "clsx";
import {useSettingsStore} from "../../stores/settings-store.ts";
import {getCallStateColors} from "../../utils/call-state-colors.ts";
import {useBlinkStore} from "../../stores/blink-store.ts";

type DAKeyProps = {
    client: ClientInfo;
    config: ClientPageConfig | undefined;
};

function DirectAccessClientKey({client, config}: DAKeyProps) {
    const blink = useBlinkStore(state => state.blink);
    const callDisplay = useCallStore(state => state.callDisplay);
    const incomingCalls = useCallStore(state => state.incomingCalls);
    const {endCall, dismissRejectedCall, dismissErrorCall} = useCallStore(state => state.actions);
    const enablePrio = useSettingsStore(state => state.callConfig.enablePriorityCalls);

    const incomingCall = incomingCalls.find(call => call.source.clientId === client.id);
    const isCalling = incomingCall !== undefined;
    const beingCalled =
        callDisplay?.type === "outgoing" && callDisplay.call.target.client === client.id;
    const involved =
        callDisplay !== undefined &&
        (callDisplay.call.target.client === client.id ||
            callDisplay.call.source.clientId === client.id);
    const inCall = callDisplay?.type === "accepted" && involved;
    const isRejected = callDisplay?.type === "rejected" && involved;
    const isError = callDisplay?.type === "error" && involved;

    const handleClick = useAsyncDebounce(async () => {
        if (isCalling) {
            if (callDisplay !== undefined) return;

            try {
                await invokeStrict("signaling_accept_call", {callId: incomingCall.callId});
            } catch {}
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
            await startCall({client: client.id});
        }
    });

    const [stationName, stationType] = splitDisplayName(client.displayName);
    const showFrequency = client.frequency !== "" && config?.frequencies === "ShowAll";

    const outgoingPrio = callDisplay?.call.prio === true && enablePrio;
    const incomingPrio = incomingCall?.prio === true && enablePrio;

    const {color, highlight} = getCallStateColors({
        inCall,
        isCalling,
        beingCalled,
        isRejected,
        isError,
        outgoingPrio,
        incomingPrio,
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
