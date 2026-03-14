import {create} from "zustand/react";
import {invoke} from "../transport";
import {isError, openErrorOverlayFromUnknown} from "../error.ts";
import {ClientInfo} from "../types/client.ts";
import {PositionId} from "../types/generic.ts";
import {useProfileStore} from "./profile-store.ts";

type State = "test" | "connecting" | "connected" | "disconnected";
type ClientInfoWithoutId = Omit<ClientInfo, "id">;

type ConnectionState = {
    connectionState: State;
    info: ClientInfoWithoutId;
    positionsToSelect: PositionId[];
    terminateOverlayVisible: boolean;
    setConnectionState: (connectionState: State) => void;
    setConnectionInfo: (info: ClientInfoWithoutId) => void;
    setPositionsToSelect: (positions: PositionId[]) => void;
    setTerminateOverlayVisible: (visible: boolean) => void;
};

export const useConnectionStore = create<ConnectionState>()(set => ({
    connectionState: "disconnected",
    info: {displayName: "", positionId: undefined, frequency: ""},
    positionsToSelect: [],
    terminateOverlayVisible: false,
    setConnectionState: connectionState => set({connectionState}),
    setConnectionInfo: info => set({info}),
    setTerminateOverlayVisible: visible => set({terminateOverlayVisible: visible}),
    setPositionsToSelect: positions => set({positionsToSelect: positions}),
}));

export const connect = async (position?: PositionId) => {
    const {setConnectionState, setTerminateOverlayVisible} = useConnectionStore.getState();
    const resetProfileStore = useProfileStore.getState().reset;

    resetProfileStore();
    setConnectionState("connecting");
    try {
        await invoke("signaling_connect", {positionId: position});
    } catch (e) {
        // Suppress error overlay on ambiguous position login error -> signaling:ambiguous-position
        if (
            isError(e) &&
            e.detail ===
                "Login failed: Multiple VATSIM positions matched your current position. Please select the correct position manually."
        ) {
            return;
        }

        setConnectionState("disconnected");

        if (
            isError(e) &&
            (e.detail === "Login failed: Another client with your CID is already connected." ||
                e.detail === "Already connected")
        ) {
            setTerminateOverlayVisible(true);
            return;
        }

        openErrorOverlayFromUnknown(e);
    }
};
