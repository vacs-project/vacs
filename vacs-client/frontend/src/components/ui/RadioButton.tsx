import Button from "./Button.tsx";
import {clsx} from "clsx";
import {useRadioState} from "../../hooks/radio-state-hook.ts";
import {useProfileType} from "../../stores/profile-store.ts";
import {navigate} from "wouter/use-browser-location";

function RadioButton() {
    const {radioState, handleButtonClick: reconnect} = useRadioState();
    const disabled = radioState.state === "NotConfigured" || radioState.state === "Disconnected";
    const textMuted = radioState.state === "NotConfigured";

    const collapsed = useProfileType() === "tabbed";

    const buttonColor = () => {
        switch (radioState.state) {
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
        navigate("/radio");
        void reconnect();
    };

    return (
        <Button
            color={buttonColor()}
            disabled={radioState.state === "NotConfigured"}
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
