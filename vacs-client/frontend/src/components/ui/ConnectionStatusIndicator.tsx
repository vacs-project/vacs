import {useCallStore} from "../../stores/call-store.ts";
import {useConnectionStore} from "../../stores/connection-store.ts";
import StatusIndicator, {Status} from "./StatusIndicator.tsx";

function ConnectionStatusIndicator() {
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

    return <StatusIndicator status={status} />;
}

export default ConnectionStatusIndicator;
