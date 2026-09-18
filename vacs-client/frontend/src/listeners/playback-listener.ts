import {toggleSayAgain} from "../stores/playback-store.ts";
import {isTauri, listen, UnlistenFn} from "../transport";

export function setupPlaybackListener() {
    const unlistenFns: Promise<UnlistenFn>[] = [];

    if (isTauri) {
        unlistenFns.push(listen<null>("playback:say-again", () => void toggleSayAgain()));
    }

    return () => {
        unlistenFns.forEach(fn => fn.then(f => f()));
    };
}
