import {clsx} from "clsx";
import {useCallStore} from "../../stores/call-store.ts";
import {useConnectionStore} from "../../stores/connection-store.ts";

export type Status = "green" | "yellow" | "red" | "gray" | "blue";

export const StatusColors: Record<Status, string> = {
    green: "bg-green-600 border-green-700",
    yellow: "bg-yellow-500 border-yellow-600",
    red: "bg-red-400 border-red-700",
    gray: "bg-gray-400 border-gray-600",
    blue: "bg-blue-500 border-blue-600",
};

function StatusIndicator() {
    const connected = useConnectionStore(state => state.connectionState === "connected");
    const callConnectionState = useCallStore(state => state.callDisplay?.connectionState);
    const status = ((): Status => {
        if (connected) {
            if (callConnectionState === "connecting" || callConnectionState === "disconnected") {
                return "yellow";
            }

            return "green";
        }

        return "gray";
    })();

    return <div className={clsx("h-3 w-3 rounded-full border", StatusColors[status])}></div>;
}

export default StatusIndicator;
