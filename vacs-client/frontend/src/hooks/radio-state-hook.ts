import {useEffect, useState} from "preact/hooks";
import {RadioState} from "../types/radio.ts";
import {invokeStrict} from "../error.ts";
import {listen} from "../transport";
import {useAsyncDebounce} from "./debounce-hook.ts";

export function useRadioState() {
    const [state, setState] = useState<RadioState>("NotConfigured");

    useEffect(() => {
        const fetchState = async () => {
            try {
                const state = await invokeStrict<RadioState>("keybinds_get_radio_state");
                setState(state);
            } catch {}
        };

        void fetchState();

        const unlisten = listen<RadioState>("radio:state", event => {
            setState(event.payload);
        });

        return () => {
            unlisten.then(fn => fn());
        };
    }, []);

    const canReconnect =
        state !== "NotConfigured" && (state === "Disconnected" || state === "Error");

    const handleButtonClick = useAsyncDebounce(async () => {
        if (canReconnect) {
            await invokeStrict("keybinds_reconnect_radio");
        }
    });

    return {state, canReconnect, handleButtonClick};
}
