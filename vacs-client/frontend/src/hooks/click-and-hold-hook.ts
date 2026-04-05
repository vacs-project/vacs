import {useCallback, useEffect, useRef} from "preact/hooks";

type UseClickAndHoldOptions = {
    enabled?: boolean;
    delayMs?: number;
    intervalMs?: number;
    onHoldTick: () => void;
    onStop?: () => void;
};

type UseClickAndHoldResult = {
    startHold: () => void;
    stopHold: () => void;
    isHoldingRef: {current: boolean};
};

export function useClickAndHold({
    enabled = true,
    delayMs = 200,
    intervalMs = 50,
    onHoldTick,
    onStop,
}: UseClickAndHoldOptions): UseClickAndHoldResult {
    const timeoutRef = useRef<number | undefined>(undefined);
    const intervalRef = useRef<number | undefined>(undefined);
    const isHoldingRef = useRef<boolean>(false);

    const enabledRef = useRef<boolean>(enabled);
    const delayMsRef = useRef<number>(delayMs);
    const intervalMsRef = useRef<number>(intervalMs);
    const onHoldTickRef = useRef<() => void>(onHoldTick);
    const onStopRef = useRef<(() => void) | undefined>(onStop);

    useEffect(() => {
        enabledRef.current = enabled;
        delayMsRef.current = delayMs;
        intervalMsRef.current = intervalMs;
        onHoldTickRef.current = onHoldTick;
        onStopRef.current = onStop;
    }, [delayMs, enabled, intervalMs, onHoldTick, onStop]);

    const clearTimers = useCallback(() => {
        if (timeoutRef.current !== undefined) {
            clearTimeout(timeoutRef.current);
            timeoutRef.current = undefined;
        }

        if (intervalRef.current !== undefined) {
            clearInterval(intervalRef.current);
            intervalRef.current = undefined;
        }
    }, []);

    const stopHold = useCallback(() => {
        clearTimers();

        if (isHoldingRef.current) {
            isHoldingRef.current = false;
            onStopRef.current?.();
        }
    }, [clearTimers]);

    const startHold = useCallback(() => {
        if (!enabledRef.current) return;

        stopHold();

        isHoldingRef.current = true;

        timeoutRef.current = setTimeout(() => {
            intervalRef.current = setInterval(() => {
                onHoldTickRef.current();
            }, intervalMsRef.current);
            timeoutRef.current = undefined;
        }, delayMsRef.current);
    }, [stopHold]);

    useEffect(() => {
        window.addEventListener("mouseup", stopHold);
        window.addEventListener("touchend", stopHold);
        window.addEventListener("touchcancel", stopHold);

        return () => {
            window.removeEventListener("mouseup", stopHold);
            window.removeEventListener("touchend", stopHold);
            window.removeEventListener("touchcancel", stopHold);
            stopHold();
        };
    }, [stopHold]);

    return {startHold, stopHold, isHoldingRef};
}
