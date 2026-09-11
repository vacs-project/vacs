import {listen, UnlistenFn} from "../transport";
import {RadioState} from "../types/radio.ts";
import {useRadioStore} from "../stores/radio-store.ts";
import {usePlaybackStore} from "../stores/playback-store.ts";
import {invokeStrict} from "../error.ts";

export function setupRadioListener() {
    const {setRadioState} = useRadioStore.getState();

    const unlistenFns: Promise<UnlistenFn>[] = [];

    const init = () => {
        unlistenFns.push(
            listen<RadioState>("radio:state", event => {
                setRadioState(event.payload);
                if (
                    event.payload.state === "Disconnected" ||
                    event.payload.state === "NotConfigured"
                ) {
                    usePlaybackStore.getState().actions.setStatus(undefined);
                }
            }),
        );
    };

    init();

    return () => {
        unlistenFns.forEach(fn => fn.then(f => f()));
    };
}

export async function fetchRadioState() {
    try {
        const state = await invokeStrict<RadioState>("radio_get_state");
        useRadioStore.getState().setRadioState(state);
    } catch {}
}
