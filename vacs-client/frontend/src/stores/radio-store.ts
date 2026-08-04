import {create} from "zustand/react";
import {startBlink, tryStopBlink} from "./blink-store.ts";
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
            tryStopBlink(null, null, false, null, null);
        }

        set({cpl});
    },
}));
