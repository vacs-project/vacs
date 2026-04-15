import {useEffect, useState} from "preact/hooks";
import {RadioState} from "../types/radio.ts";
import {invokeStrict} from "../error.ts";
import {listen} from "../transport";
import {useAsyncDebounce} from "./debounce-hook.ts";

export function useRadioState() {
    const [radioState, setRadioState] = useState<RadioState>({state: "NotConfigured"});

    useEffect(() => {
        const fetchState = async () => {
            try {
                const state = await invokeStrict<RadioState>("keybinds_get_radio_state");
                setRadioState(state);
            } catch {}
        };

        void fetchState();

        const unlisten = listen<RadioState>("radio:state", event => {
            setRadioState(event.payload);
        });

        return () => {
            void unlisten.then(fn => fn());
        };
    }, []);

    const canReconnect =
        radioState.state !== "NotConfigured" &&
        (radioState.state === "Disconnected" || radioState.state === "Error");

    const handleButtonClick = useAsyncDebounce(async () => {
        if (canReconnect) {
            await invokeStrict("keybinds_reconnect_radio");
        }
    });

    return {radioState, canReconnect, handleButtonClick};
}
