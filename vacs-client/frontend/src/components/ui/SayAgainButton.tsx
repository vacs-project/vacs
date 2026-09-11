import {clsx} from "clsx";
import {useSayAgain} from "../../hooks/say-again-hook.ts";
import {useCapabilitiesStore} from "../../stores/capabilities-store.ts";
import {useRadioStore} from "../../stores/radio-store.ts";
import {useSettingsStore} from "../../stores/settings-store.ts";
import Button from "./Button.tsx";

function SayAgainButton() {
    const capPlayback = useCapabilitiesStore(state => state.playback);
    const radioEnabled = useSettingsStore(state => state.radioConfig?.integration != null);
    const playbackEnabled = useSettingsStore(state => state.playbackEnabled);
    const radioConnected = useRadioStore(
        state =>
            state.radioState?.state !== "NotConfigured" &&
            state.radioState?.state !== "Disconnected",
    );

    const {active, handlePress} = useSayAgain();

    const textMuted = !capPlayback || !radioEnabled || !playbackEnabled;
    const disabled = textMuted || !radioConnected;

    return (
        <Button
            color={active ? "blue" : "cyan"}
            className={clsx(textMuted && "text-slate-400")}
            onClick={handlePress}
            disabled={disabled}
        >
            <p>
                SAY
                <br />
                AGAIN
            </p>
        </Button>
    );
}

export default SayAgainButton;
