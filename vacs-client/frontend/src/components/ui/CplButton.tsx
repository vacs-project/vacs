import Button from "./Button.tsx";
import {useRadioStore} from "../../stores/radio-store.ts";
import {clsx} from "clsx";
import {useBlinkStore} from "../../stores/blink-store.ts";
import {useSettingsStore} from "../../stores/settings-store.ts";

function CplButton() {
    const blink = useBlinkStore(state => state.blink);
    const cpl = useRadioStore(state => state.cpl);
    const radioState = useRadioStore(state => state.radioState?.state ?? "NotConfigured");
    const setCpl = useRadioStore(state => state.setCpl);
    const radioIntegration = useSettingsStore(state => state.radioConfig?.integration);

    const disabled =
        radioState === "NotConfigured" ||
        radioState === "Disconnected" ||
        radioIntegration !== "TrackAudio";
    const textMuted = radioState === "NotConfigured" || radioIntegration !== "TrackAudio";

    return (
        <Button
            color={cpl ? (blink ? "blue" : "cyan") : "cyan"}
            className={clsx("text-lg", textMuted && "text-gray-500")}
            onClick={() => setCpl(!cpl)}
            disabled={disabled}
        >
            CPL
        </Button>
    );
}

export default CplButton;
