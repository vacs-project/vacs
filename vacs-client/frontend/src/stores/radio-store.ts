import {create} from "zustand/react";
import {startBlink, tryStopBlink} from "./blink-store.ts";
import {invokeStrict} from "../error.ts";
import {RadioState} from "../types/radio.ts";

type RadioStoreState = {
    radioState: RadioState | undefined;
    cpl: boolean;
    setRadioState: (state: RadioState) => void;
    setCpl: (cpl: boolean) => void;
};

export const useRadioStore = create<RadioStoreState>()(set => ({
    cpl: false,
    radioState: undefined,
    setRadioState: state => set({radioState: state}),
    setCpl: cpl => {
        if (cpl) {
            startBlink();
        } else {
            tryStopBlink(null, null, false, null);
        }

        set({cpl});
    },
}));

export const selectRadioConnected = (state: RadioStoreState) =>
    state.radioState?.state !== "NotConfigured" && state.radioState?.state !== "Disconnected";

/**
 * Reconnects the radio if it is currently disconnected or in an error state.
 */
export const retryRadioConnection = () => {
    const state = useRadioStore.getState().radioState?.state;

    if (state === "Disconnected" || state === "Error") {
        void invokeStrict("radio_reconnect");
    }
};
