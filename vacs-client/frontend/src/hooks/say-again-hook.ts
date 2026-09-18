import {useEffect} from "preact/hooks";
import {toggleSayAgain, usePlaybackStore} from "../stores/playback-store.ts";
import {EventCallback, isTauri, listen} from "../transport";
import {useAsyncDebounce} from "./debounce-hook.ts";
import {useEventCallback} from "./event-callback-hook.ts";

export function useSayAgain() {
    const active = usePlaybackStore(state => state.status?.sayAgain === true);
    const setStatus = usePlaybackStore(state => state.actions.setStatus);

    const handlePress = useAsyncDebounce(toggleSayAgain);

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
