import Button from "./ui/Button.tsx";
import {useCallStore} from "../stores/call-store.ts";
import {invokeStrict} from "../error.ts";
import unplug from "../assets/unplug.svg";
import {Call} from "../types/call.ts";
import {useProfileStationKeys} from "../stores/profile-store.ts";
import {DirectAccessKey} from "../types/profile.ts";
import {ComponentChild} from "preact";
import {ClientId, PositionId, StationId} from "../types/generic.ts";
import {useAuthStore} from "../stores/auth-store.ts";
import {ClientInfo, splitDisplayName} from "../types/client.ts";
import {clsx} from "clsx";
import ButtonLabel from "./ui/ButtonLabel.tsx";
import {useClientsStore} from "../stores/clients-store.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import {getCallStateColors} from "../utils/call-state-colors.ts";
import {useBlinkStore} from "../stores/blink-store.ts";

function CallQueue() {
    const blink = useBlinkStore(state => state.blink);
    const callDisplay = useCallStore(state => state.callDisplay);
    const incomingCalls = useCallStore(state => state.incomingCalls);
    const {endCall, dismissRejectedCall, dismissErrorCall, removeCall} = useCallStore(
        state => state.actions,
    );
    const stationKeys = useProfileStationKeys();
    const cid = useAuthStore(state => state.cid);
    const clients = useClientsStore(state => state.clients);
    const enablePrio = useSettingsStore(state => state.callConfig.enablePriorityCalls);

    const handleCallDisplayClick = async (call: Call) => {
        if (callDisplay?.type === "accepted" || callDisplay?.type === "outgoing") {
            try {
                await invokeStrict("signaling_end_call", {callId: call.callId});
                endCall();
            } catch {}
        } else if (callDisplay?.type === "rejected") {
            dismissRejectedCall();
        } else if (callDisplay?.type === "error") {
            dismissErrorCall();
        }
    };

    const handleAnswerKeyClick = async (call: Call) => {
        // Can't accept someone's call if something is in your call display
        if (callDisplay !== undefined) return;

        try {
            await invokeStrict("signaling_accept_call", {callId: call.callId});
        } catch {
            removeCall(call.callId);
        }
    };

    const cdPrio = callDisplay?.call.prio === true && enablePrio;

    const {color: cdColor, highlight: cdHighlight} = getCallStateColors({
        inCall: callDisplay?.type === "accepted",
        isCalling: false,
        beingCalled: callDisplay?.type === "outgoing",
        isRejected: callDisplay?.type === "rejected",
        isError: callDisplay?.type === "error",
        outgoingPrio: cdPrio,
        incomingPrio: false,
        blink,
    });

    return (
        <div
            className="flex flex-col-reverse gap-2.5 pt-3 pr-px overflow-y-auto [&>button]:shrink-0"
            style={{scrollbarWidth: "none"}}
        >
            {/*Call Display*/}
            {callDisplay !== undefined ? (
                <div className="relative">
                    {callDisplay.connectionState === "disconnected" && (
                        <img
                            className="absolute top-1 left-1 h-5 w-5"
                            src={unplug}
                            alt="Disconnected"
                        />
                    )}
                    <Button
                        color={cdColor}
                        highlight={cdHighlight}
                        softDisabled={true}
                        onClick={() => handleCallDisplayClick(callDisplay.call)}
                        className={clsx(
                            "h-16 text-sm [&_p]:leading-3.5",
                            cdColor === "gray" ? "p-1.5" : "p-[calc(0.375rem+1px)]",
                        )}
                    >
                        {callDisplayLabel(callDisplay.call, cid, stationKeys, clients)}
                    </Button>
                </div>
            ) : (
                <div className="w-full h-16 shrink-0 border rounded-md"></div>
            )}

            {/*Answer Keys*/}
            {incomingCalls.map((call, idx) => {
                const incomingPrio = call.prio && enablePrio;
                const {color, highlight} = getCallStateColors({
                    inCall: false,
                    isCalling: true,
                    beingCalled: false,
                    isRejected: false,
                    isError: false,
                    outgoingPrio: false,
                    incomingPrio,
                    blink,
                });
                return (
                    <Button
                        key={idx}
                        color={color}
                        highlight={highlight}
                        className={clsx(
                            "h-16 text-sm [&_p]:leading-3.5",
                            color === "gray" ? "p-1.5" : "p-[calc(0.375rem+1px)]",
                        )}
                        onClick={() => handleAnswerKeyClick(call)}
                    >
                        {callLabel(
                            call.source.stationId,
                            call.source.positionId,
                            call.source.clientId,
                            stationKeys,
                            clients,
                        )}
                    </Button>
                );
            })}
            {Array.from(Array(Math.max(5 - incomingCalls.length, 0)).keys()).map(idx => (
                <div key={idx} className="w-full h-16 shrink-0 border rounded-md"></div>
            ))}
        </div>
    );
}

function callDisplayLabel(
    call: Call,
    cid: ClientId | undefined,
    stationKeys: DirectAccessKey[],
    clients: ClientInfo[],
): ComponentChild {
    return call.source.clientId === cid
        ? callLabel(
              call.target.station,
              call.target.position,
              call.target.client,
              stationKeys,
              clients,
          )
        : callLabel(
              call.source.stationId,
              call.source.positionId,
              call.source.clientId,
              stationKeys,
              clients,
          );
}

const callLabel = (
    stationId: StationId | undefined,
    positionId: PositionId | undefined,
    clientId: ClientId | undefined,
    stationKeys: DirectAccessKey[],
    clients: ClientInfo[],
): ComponentChild => {
    if (stationId !== undefined) {
        const station = stationKeys.find(key => key.stationId === stationId);
        if (station !== undefined) {
            return <ButtonLabel label={station.label} />;
        }
        return callsignLabel(stationId);
    } else if (positionId !== undefined) {
        return callsignLabel(positionId);
    }
    return callsignLabel(
        clientId !== undefined
            ? (clients.find(client => client.id === clientId)?.displayName ?? clientId)
            : "",
    );
};

function callsignLabel(name: string): ComponentChild {
    const [stationName, stationType] = splitDisplayName(name);
    return (
        <>
            <p className="max-w-full truncate" title={name}>
                {stationName}
            </p>
            {stationType !== "" && <p>{stationType}</p>}
        </>
    );
}

export default CallQueue;
