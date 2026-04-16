import Button from "./Button.tsx";
import {useRadioStore} from "../../stores/radio-store.ts";
import {useRadioState} from "../../hooks/radio-state-hook.ts";
import {clsx} from "clsx";
import {useBlinkStore} from "../../stores/blink-store.ts";

function CplButton() {
    const blink = useBlinkStore(state => state.blink);
    const cpl = useRadioStore(state => state.cpl);
    const setCpl = useRadioStore(state => state.setCpl);
    const {radioState} = useRadioState();
    const disabled = radioState.state === "NotConfigured" || radioState.state === "Disconnected";
    const textMuted = radioState.state === "NotConfigured";

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
