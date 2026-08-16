import {create} from "zustand/react";
import {CallDisplay, ConferenceState, useCallStore} from "./call-store.ts";
import {useRadioStore} from "./radio-store.ts";
import {isPlaybackPaused} from "./playback-store.ts";

type BlinkState = {
    blink: boolean;
    blinkTimeoutId: number | undefined;
    startBlink: () => void;
    tryStopBlink: (
        incomingCallsLength: number | null,
        callDisplay: CallDisplay | undefined | null,
        cpl: boolean | null,
        playbackPaused: boolean | null,
        conferenceState: ConferenceState | null,
    ) => void;
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
    tryStopBlink: (incomingCallsLength, callDisplay, cpl, playbackPaused, conferenceState) => {
        if (get().blinkTimeoutId === undefined) return;

        const effectiveCallsLength =
            incomingCallsLength ?? useCallStore.getState().incomingCalls.length;
        const effectiveCallDisplay =
            callDisplay === null ? useCallStore.getState().callDisplay : callDisplay;
        const effectiveCpl = cpl ?? useRadioStore.getState().cpl;
        const effectivePlaybackPaused = playbackPaused ?? isPlaybackPaused();
        const effectiveConferenceState = conferenceState ?? useCallStore.getState().conferenceState;

        if (
            !shouldStopBlinking(
                effectiveCallsLength,
                effectiveCallDisplay,
                effectiveCpl,
                effectivePlaybackPaused,
                effectiveConferenceState,
            )
        ) {
            return;
        }

        clearTimeout(get().blinkTimeoutId);
        set({blink: false, blinkTimeoutId: undefined});
    },
    stopBlink: () => {
        if (get().blinkTimeoutId === undefined) return;
        clearTimeout(get().blinkTimeoutId);
        set({blink: false, blinkTimeoutId: undefined});
    },
}));

export const startBlink = () => useBlinkStore.getState().startBlink();
export const tryStopBlink: BlinkState["tryStopBlink"] = (
    incomingCallsLength,
    callDisplay,
    cpl,
    playbackPaused,
    conferenceState,
) =>
    useBlinkStore
        .getState()
        .tryStopBlink(incomingCallsLength, callDisplay, cpl, playbackPaused, conferenceState);
export const stopBlink = () => useBlinkStore.getState().stopBlink();
export const syncBlink = () => {
    const {incomingCalls, callDisplay, conferenceState} = useCallStore.getState();
    if (
        shouldStopBlinking(
            incomingCalls.length,
            callDisplay,
            useRadioStore.getState().cpl,
            isPlaybackPaused(),
            conferenceState,
        )
    ) {
        useBlinkStore.getState().stopBlink();
    } else {
        useBlinkStore.getState().startBlink();
    }
};

const shouldStopBlinking = (
    incomingCallsLength: number,
    callDisplay: CallDisplay | undefined,
    cpl: boolean,
    playbackPaused: boolean,
    conferenceState: ConferenceState,
) => {
    return (
        !cpl &&
        !playbackPaused &&
        incomingCallsLength === 0 &&
        conferenceState !== "modify" &&
        (callDisplay === undefined ||
            (callDisplay.type !== "rejected" &&
                callDisplay.type !== "error" &&
                callDisplay.rejectedTargets.length !== 0 &&
                callDisplay.erroredTargets.length !== 0 &&
                callDisplay.type === "accepted") ||
            (callDisplay.type === "outgoing" && !callDisplay.call.prio))
    );
};
