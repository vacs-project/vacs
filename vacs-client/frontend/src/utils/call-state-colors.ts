import {ButtonColor, ButtonHighlightColor} from "../components/ui/Button.tsx";
import {CustomButtonColor} from "../types/custom-button-colors.ts";

export type CallStateColorParams = {
    inCall: boolean;
    isCalling: boolean;
    beingCalled: boolean;
    isRejected: boolean;
    isError: boolean;
    isTarget?: boolean;
    prio: boolean;
    blink: boolean;
    temporarySource?: boolean;
    defaultSource?: boolean;
    defaultColor?: CustomButtonColor;
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
    prio,
    blink,
    temporarySource = false,
    defaultSource = false,
    defaultColor,
}: CallStateColorParams): CallStateColors {
    const backgroundColor: ButtonHighlightColor = defaultColor ?? "gray";

    let color: ButtonColor;

    if (inCall) {
        color = prio ? "yellow" : "green";
    } else if (isCalling) {
        if (blink) {
            color = prio ? "yellow" : "green";
        } else {
            color = backgroundColor;
        }
    } else if (beingCalled && prio) {
        color = blink ? "yellow" : backgroundColor;
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
        color = backgroundColor;
    }

    let highlight: ButtonHighlightColor | undefined;

    if (isCalling && prio) {
        highlight = blink ? "green" : backgroundColor;
    } else if ((beingCalled && !isTarget) || isRejected || (inCall && prio)) {
        highlight = "green";
    } else {
        highlight = undefined;
    }

    return {color, highlight};
}
