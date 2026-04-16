import Button from "./Button.tsx";
import {useRadioStore} from "../../stores/radio-store.ts";
import {clsx} from "clsx";
import {useBlinkStore} from "../../stores/blink-store.ts";

function CplButton() {
    const blink = useBlinkStore(state => state.blink);
    const cpl = useRadioStore(state => state.cpl);
    const radioState = useRadioStore(state => state.radioState?.state ?? "NotConfigured");
    const setCpl = useRadioStore(state => state.setCpl);
    const disabled = radioState === "NotConfigured" || radioState === "Disconnected";
    const textMuted = radioState === "NotConfigured";

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
