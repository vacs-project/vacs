import {ButtonColor, ButtonHighlightColor} from "../components/ui/Button.tsx";

export type CallStateColorParams = {
    inCall: boolean;
    isCalling: boolean;
    beingCalled: boolean;
    isRejected: boolean;
    isError: boolean;
    isTarget?: boolean;
    outgoingPrio: boolean;
    incomingPrio: boolean;
    blink: boolean;
    temporarySource?: boolean;
    defaultSource?: boolean;
    defaultColor?: ButtonColor;
};

export type CallStateColors = {
    color: ButtonColor;
    highlight: ButtonHighlightColor | undefined;
};

export function getCallStateColors({
    inCall,
    isCalling,
    beingCalled,
    isRejected,
    isError,
    isTarget = false,
    outgoingPrio,
    incomingPrio,
    blink,
    temporarySource = false,
    defaultSource = false,
    defaultColor = "gray",
}: CallStateColorParams): CallStateColors {
    let color: ButtonColor;

    if (inCall) {
        color = outgoingPrio ? "yellow" : "green";
    } else if (isCalling) {
        if (blink) {
            color = incomingPrio ? "yellow" : "green";
        } else {
            color = defaultColor;
        }
    } else if (beingCalled && outgoingPrio) {
        color = blink ? "yellow" : defaultColor;
    } else if (isRejected && blink) {
        color = "green";
    } else if (isError && blink) {
        color = "red";
    } else if (isTarget) {
        color = "sage";
    } else if (temporarySource) {
        color = "peach";
    } else if (defaultSource) {
        color = "honey";
    } else {
        color = defaultColor;
    }

    let highlight: ButtonHighlightColor | undefined;

    if (isCalling && incomingPrio) {
        highlight = blink ? "green" : "gray";
    } else if (beingCalled || isRejected || (inCall && outgoingPrio)) {
        highlight = "green";
    } else {
        highlight = undefined;
    }

    return {color, highlight};
}
