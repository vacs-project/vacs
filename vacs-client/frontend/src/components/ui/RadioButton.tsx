import {clsx} from "clsx";
import {goToPage} from "../../stores/navigation-store.ts";
import {useProfileType} from "../../stores/profile-store.ts";
import {retryRadioConnection, useRadioStore} from "../../stores/radio-store.ts";
import {useSettingsStore} from "../../stores/settings-store.ts";
import Button from "./Button.tsx";

function RadioButton() {
    const radioState = useRadioStore(state => state.radioState?.state ?? "NotConfigured");
    const textMuted = radioState === "NotConfigured";
    const radioIsTrackAudio = useSettingsStore(
        state => state.radioConfig?.integration === "TrackAudio",
    );

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
        if (radioIsTrackAudio) {
            goToPage("radio");
        }

        retryRadioConnection();
    };

    return (
        <Button
            color={buttonColor()}
            disabled={radioState === "NotConfigured"}
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
