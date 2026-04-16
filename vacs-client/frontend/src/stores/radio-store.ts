import {create} from "zustand/react";
import {shouldStopBlinking, startBlink, stopBlink} from "./blink-store.ts";
import {useCallStore} from "./call-store.ts";
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
            const {incomingCalls, callDisplay} = useCallStore.getState();
            if (shouldStopBlinking(incomingCalls.length, callDisplay, false)) {
                stopBlink();
            }
        }

        set({cpl});
    },
}));
