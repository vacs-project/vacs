import {create} from "zustand/react";

type ErrorOverlayState = {
    visible: boolean;
    title: string;
    message: string;
    isNonCritical: boolean;
    dismissable: boolean;
    timeoutId?: number;
    open: (
        title: string,
        message: string,
        isNonCritical: boolean,
        timeout?: number,
        dismissable?: boolean,
    ) => void;
    close: () => void;
    closeIfTitle: (title: string) => void;
};

const CLOSED_OVERLAY: Omit<ErrorOverlayState, "open" | "close" | "closeIfTitle"> = {
    visible: false,
    title: "",
    message: "",
    isNonCritical: false,
    dismissable: true,
    timeoutId: undefined,
};

export const useErrorOverlayStore = create<ErrorOverlayState>()((set, get) => ({
    visible: false,
    title: "",
    message: "",
    isNonCritical: false,
    dismissable: true,
    timeoutId: undefined,
    open: (title, message, isNonCritical, timeoutMs, dismissable = true) => {
        const previous_timeout_id = get().timeoutId;
        if (previous_timeout_id !== undefined) {
            clearTimeout(previous_timeout_id);
        }

        const timeout_id =
            timeoutMs !== undefined ? setTimeout(() => set(CLOSED_OVERLAY), timeoutMs) : undefined;

        set({visible: true, title, message, isNonCritical, dismissable, timeoutId: timeout_id});
    },
    close: () => {
        const timeout_id = get().timeoutId;
        if (timeout_id !== undefined) {
            clearTimeout(timeout_id);
        }

        set(CLOSED_OVERLAY);
    },
    closeIfTitle: (title: string) => {
        if (!get().visible || get().title !== title) return;
        get().close();
    },
}));
