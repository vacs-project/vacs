import {allConnectionStates, useCallStore} from "../../stores/call-store.ts";
import {useConnectionStore} from "../../stores/connection-store.ts";
import StatusIndicator, {Status} from "./StatusIndicator.tsx";

function ConnectionStatusIndicator() {
    const connected = useConnectionStore(state => state.connectionState === "connected");
    const allConnected = useCallStore(state => allConnectionStates(state.callDisplay, "connected"));
    const status = ((): Status => {
        if (connected) {
            return allConnected ? "green" : "yellow";
        }

        return "gray";
    })();

    return <StatusIndicator status={status} />;
}

export default ConnectionStatusIndicator;
