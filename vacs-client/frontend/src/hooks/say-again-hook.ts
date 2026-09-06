import {useEffect} from "preact/hooks";
import {invokeStrict} from "../error.ts";
import {usePlaybackStore} from "../stores/playback-store.ts";
import {EventCallback, isTauri, listen} from "../transport";
import {ClipMeta} from "../types/playback.ts";
import {useAsyncDebounce} from "./debounce-hook.ts";
import {useEventCallback} from "./event-callback-hook.ts";

export function useSayAgain() {
    const active = usePlaybackStore(state => state.status?.sayAgain === true);
    const playbackDevice = usePlaybackStore(state => state.playbackDevice);
    const setStatus = usePlaybackStore(state => state.actions.setStatus);

    const handlePress = useAsyncDebounce(async () => {
        if (active) {
            try {
                await invokeStrict("playback_stop");
                setStatus(undefined);
            } catch {}
            return;
        }

        try {
            const clip = await invokeStrict<ClipMeta | null>("playback_say_again", {
                deviceType: playbackDevice,
            });
            if (clip === null) return;
            setStatus({
                id: clip.id,
                status: "playing",
                continuously: false,
                sayAgain: true,
                progress: 0,
            });
        } catch {
            setStatus(undefined);
        }
    });

    // An open playback page owns the progress stream and clears the status itself.
    // Without one, the desktop instance does it here and syncs the result to remotes.
    const handleProgress: EventCallback<number> = useEventCallback(event => {
        if (event.payload < 1) return;
        const {status, openInstanceIds} = usePlaybackStore.getState();
        if (status?.sayAgain !== true || openInstanceIds.length > 0 || !isTauri) return;
        setStatus(undefined);
    });

    useEffect(() => {
        const unlisten = listen<number>("playback:progress", handleProgress);
        return () => unlisten.then(fn => fn());
    }, [handleProgress]);

    return {active, handlePress};
}
