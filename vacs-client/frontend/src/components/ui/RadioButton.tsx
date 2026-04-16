import Button from "./Button.tsx";
import {clsx} from "clsx";
import {useProfileType} from "../../stores/profile-store.ts";
import {navigate} from "wouter/use-browser-location";
import {useRadioStore} from "../../stores/radio-store.ts";
import {invokeStrict} from "../../error.ts";

function RadioButton() {
    const radioState = useRadioStore(state => state.radioState?.state ?? "NotConfigured");
    const disabled = radioState === "NotConfigured" || radioState === "Disconnected";
    const textMuted = radioState === "NotConfigured";

    const collapsed = useProfileType() === "tabbed";

    const buttonColor = () => {
        switch (radioState) {
            case "NotConfigured":
            case "Disconnected":
                return "gray";
            case "Connected":
            case "VoiceConnected":
                return "gray";
            case "RxIdle":
                return "emerald";
            case "RxActive":
                return "cornflower";
            case "TxActive":
                return "cornflower";
            case "Error":
                return "red";
            default:
                return "gray";
        }
    };

    const handleButtonClick = () => {
        if (!disabled) {
            navigate("/radio");
        }

        if (
            radioState !== "NotConfigured" &&
            (radioState === "Disconnected" || radioState === "Error")
        ) {
            void invokeStrict("keybinds_reconnect_radio");
        }
    };

    return (
        <Button
            color={buttonColor()}
            disabled={radioState === "NotConfigured"}
            softDisabled={disabled}
            onClick={handleButtonClick}
            className={clsx(
                "text-lg transition-[width]",
                textMuted && "text-gray-500",
                collapsed ? "w-24" : "w-46",
            )}
        >
            Radio
        </Button>
    );
}

export default RadioButton;
