import {useCallback, useEffect, useRef} from "preact/hooks";
import {useShallow} from "zustand/react/shallow";
import {invokeStrict} from "../error.ts";
import {isPlaybackRoot, PlaybackStatusBase, usePlaybackStore} from "../stores/playback-store.ts";
import {EventCallback, listen} from "../transport";
import {ClipMeta} from "../types/playback.ts";
import {useAsyncDebounce} from "./debounce-hook.ts";
import {useEventCallback} from "./event-callback-hook.ts";
import {PlaybackDeviceType} from "../types/audio.ts";

type Params = {
    clips: ClipMeta[];
    selectedClip: ClipMeta | undefined;
    prevClip: ClipMeta | undefined;
    nextClip: ClipMeta | undefined;
};

export function usePlaybackControls({clips, selectedClip, prevClip, nextClip}: Params) {
    const status = usePlaybackStore(
        useShallow(state => {
            const s = state.status;
            if (s === undefined) return undefined;
            return {
                id: s.id,
                status: s.status,
                continuously: s.continuously,
                sayAgain: s.sayAgain,
            } satisfies PlaybackStatusBase;
        }),
    );
    const playbackDevice = usePlaybackStore(state => state.playbackDevice);
    const {setSelected, setStatus, setPlaybackDevice} = usePlaybackStore(state => state.actions);

    const intendedClipChangeRef = useRef(false);

    const handleStart = useAsyncDebounce(
        async (id: number, deviceType: PlaybackDeviceType, continuously: boolean = false) => {
            try {
                await invokeStrict("playback_start", {id, deviceType});
                setStatus({id, status: "playing", continuously, sayAgain: false, progress: 0});
            } catch {
                setStatus(undefined);
            }
        },
    );

    const handlePause = useAsyncDebounce(async () => {
        try {
            await invokeStrict("playback_pause");
            setStatus(prev => {
                if (prev === undefined) return prev;
                return {...prev, status: "paused"};
            });
        } catch {}
    });

    const handleContinue = useAsyncDebounce(async () => {
        try {
            await invokeStrict("playback_continue");
            setStatus(prev => {
                if (prev === undefined) return prev;
                return {...prev, status: "playing"};
            });
        } catch {}
    });

    const handleStop = useAsyncDebounce(
        useCallback(async () => {
            try {
                await invokeStrict("playback_stop");
                setStatus(undefined);
            } catch {}
        }, [setStatus]),
    );

    const handleSeek = useAsyncDebounce(async (durationSecs: number) => {
        const millis = 1000 * durationSecs;
        try {
            await invokeStrict("playback_seek", {millis});
            setStatus(prev => {
                if (prev?.status !== "paused" || selectedClip === undefined) return prev;
                const diff = millis / selectedClip.durationMs;
                return {
                    ...prev,
                    progress: Math.min(Math.max(prev.progress + diff, 0), 1),
                };
            });
        } catch {}
    });

    const handleDeviceSwitch = useAsyncDebounce(async () => {
        const deviceType = playbackDevice === "Output" ? "Speaker" : "Output";
        if (status === undefined) {
            setPlaybackDevice(deviceType);
        } else {
            try {
                const currentProgress = usePlaybackStore.getState().status?.progress ?? 0;
                await invokeStrict("playback_start", {
                    id: status.id,
                    deviceType,
                    initialProgress: currentProgress,
                    startPaused: status.status === "paused",
                });
                setPlaybackDevice(deviceType);
            } catch {}
        }
    });

    const handlePlayPause = async (continuously: boolean) => {
        if (status === undefined) {
            if (selectedClip !== undefined)
                void handleStart(selectedClip.id, playbackDevice, continuously);
        } else if (status.continuously === continuously) {
            if (status.status === "playing") {
                void handlePause();
            } else {
                void handleContinue();
            }
        }
    };

    const handleSeekBack = () => void handleSeek(-1);
    const handleSeekForward = () => void handleSeek(1);

    const handlePrev = async () => {
        intendedClipChangeRef.current = true;
        setSelected(prev => prev + 1);
        if (prevClip !== undefined)
            void handleStart(prevClip.id, playbackDevice, status?.continuously);
    };

    const handleNext = async () => {
        intendedClipChangeRef.current = true;
        setSelected(prev => prev - 1);
        if (nextClip !== undefined)
            void handleStart(nextClip.id, playbackDevice, status?.continuously);
    };

    // A clip started elsewhere (SAY AGAIN, another instance) moves the selection to it
    // instead of tripping the stop below.
    const statusId = status?.id;
    useEffect(() => {
        if (statusId === undefined) return;
        const {selected} = usePlaybackStore.getState();
        if (clips[selected]?.id === statusId) return;
        const index = clips.findIndex(c => c.id === statusId);
        if (index === -1) return;
        intendedClipChangeRef.current = true;
        setSelected(index);
    }, [statusId, clips, setSelected]);

    useEffect(() => {
        if (!isPlaybackRoot()) return;
        const status = usePlaybackStore.getState().status;
        if (
            selectedClip !== undefined &&
            status !== undefined &&
            status.id !== selectedClip.id &&
            !intendedClipChangeRef.current
        ) {
            void handleStop();
        }
        intendedClipChangeRef.current = false;
    }, [selectedClip, handleStop]);

    const handleProgressUpdate: EventCallback<number> = useEventCallback(event => {
        setStatus(prev => {
            if (prev === undefined) return prev;
            return {...prev, progress: event.payload};
        });
        if (event.payload === 1 && isPlaybackRoot()) {
            if (status?.continuously && nextClip !== undefined) {
                intendedClipChangeRef.current = true;
                setSelected(prev => prev - 1);
                void handleStart(nextClip.id, playbackDevice, true);
            } else {
                setStatus(undefined);
            }
        }
    });

    useEffect(() => {
        const unlisten = listen<number>("playback:progress", handleProgressUpdate);
        return () => unlisten.then(fn => fn());
    }, [handleProgressUpdate]);

    return {
        status,
        playbackDevice,
        active: status !== undefined,
        handleStop,
        handlePlayPause,
        handleSeekBack,
        handleSeekForward,
        handleDeviceSwitch,
        handlePrev,
        handleNext,
    };
}
