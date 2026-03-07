import {useErrorOverlayStore} from "../stores/error-overlay-store.ts";
import {listen, UnlistenFn} from "../transport";
import {Error} from "../error.ts";

export function setupErrorListeners() {
    const openErrorOverlay = useErrorOverlayStore.getState().open;

    const unlistenFns: Promise<UnlistenFn>[] = [];

    const init = () => {
        unlistenFns.push(
            listen<Error>("error", event => {
                openErrorOverlay(
                    event.payload.title,
                    event.payload.detail,
                    event.payload.isNonCritical,
                    event.payload.timeoutMs,
                );
            }),
        );
    };

    init();

    return () => {
        unlistenFns.forEach(fn => fn.then(f => f()));
    };
}
