import {clsx} from "clsx";
import Button from "../ui/Button.tsx";
import {useCallback, useState} from "preact/hooks";
import {invoke} from "../../transport";
import {useErrorOverlayStore} from "../../stores/error-overlay-store.ts";
import {isError, openErrorOverlayFromUnknown} from "../../error.ts";

const CALLSIGN_PATTERN = /^(?=.{4,12}$).+_[A-Z]{3}$/;

function AddRadioStation() {
    const [value, setValue] = useState<string>("");
    const [disabled, setDisabled] = useState<boolean>(true);
    const openErrorOverlay = useErrorOverlayStore(state => state.open);

    const handleAddClick = useCallback(async () => {
        if (!CALLSIGN_PATTERN.test(value)) return;

        try {
            await invoke("radio_add_station", {callsign: value});
            setValue("");
            setDisabled(true);
        } catch (e) {
            if (isError(e) && e.detail.includes("timeout")) {
                openErrorOverlay(
                    "Radio error",
                    "Either TrackAudio is not connected or the station does not exist.",
                    false,
                );
            } else {
                openErrorOverlayFromUnknown(e);
            }
        }
    }, [value, openErrorOverlay]);

    return (
        <div className="w-42 h-8 flex gap-2 items-center">
            <input
                type="text"
                id="add-station"
                className={clsx(
                    "w-full h-full px-2 py-1.5 border bg-gray-300 rounded text-sm focus:outline-none font-semibold placeholder:font-normal placeholder:text-gray-500",
                    "disabled:brightness-90 disabled:cursor-not-allowed border-gray-700 focus:border-blue-500",
                )}
                placeholder="Callsign"
                value={value}
                onKeyDown={e => {
                    if (e.key === "Enter" && !disabled) {
                        void handleAddClick();
                        e.currentTarget.blur();
                    }
                }}
                onChange={e => {
                    const newValue = e.currentTarget.value.trim().toUpperCase();
                    e.currentTarget.value = newValue;

                    setValue(newValue);
                    const valid = CALLSIGN_PATTERN.test(newValue);
                    setDisabled(!valid);
                }}
            />
            <Button
                color="gray"
                className="h-full w-8! shrink-0 flex items-center justify-center"
                onClick={handleAddClick}
                disabled={disabled}
            >
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="18"
                    height="18"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                >
                    <path d="M5 12h14" />
                    <path d="M12 5v14" />
                </svg>
            </Button>
        </div>
    );
}

export default AddRadioStation;
