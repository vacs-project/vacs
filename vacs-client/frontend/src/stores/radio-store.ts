import {create} from "zustand/react";
import {shouldStopBlinking, startBlink, stopBlink} from "./blink-store.ts";
import {useCallStore} from "./call-store.ts";

type RadioStoreState = {
    cpl: boolean;
    setCpl: (cpl: boolean) => void;
};

export const useRadioStore = create<RadioStoreState>()(set => ({
    cpl: false,
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
