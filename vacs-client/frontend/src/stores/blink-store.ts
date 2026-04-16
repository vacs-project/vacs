import {create} from "zustand/react";
import {CallDisplay} from "./call-store.ts";

type BlinkState = {
    blink: boolean;
    blinkTimeoutId: number | undefined;
    startBlink: () => void;
    stopBlink: () => void;
};

export const useBlinkStore = create<BlinkState>()((set, get) => ({
    blink: false,
    blinkTimeoutId: undefined,
    startBlink: () => {
        if (get().blinkTimeoutId !== undefined) return;
        const toggleBlink = (blink: boolean) => {
            const timeoutId = setTimeout(() => {
                toggleBlink(!blink);
            }, 500);
            set({blinkTimeoutId: timeoutId, blink: blink});
        };
        toggleBlink(true);
    },
    stopBlink: () => {
        if (get().blinkTimeoutId === undefined) return;
        clearTimeout(get().blinkTimeoutId);
        set({blink: false, blinkTimeoutId: undefined});
    },
}));

export const startBlink = () => useBlinkStore.getState().startBlink();
export const stopBlink = () => useBlinkStore.getState().stopBlink();

export const shouldStopBlinking = (
    incomingCallsLength: number,
    callDisplay: CallDisplay | undefined,
    cpl: boolean,
) => {
    return (
        !cpl &&
        incomingCallsLength === 0 &&
        (callDisplay === undefined ||
            (callDisplay.type !== "rejected" &&
                callDisplay.type !== "error" &&
                callDisplay.type === "accepted") ||
            (callDisplay.type === "outgoing" && !callDisplay.call.prio))
    );
};
