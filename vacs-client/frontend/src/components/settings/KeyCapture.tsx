import {clsx} from "clsx";
import {useCallback, useEffect, useRef, useState} from "preact/hooks";
import {invokeSafe} from "../../error.ts";

type KeyCaptureProps = {
    label: string | null;
    onCapture: (code: string) => Promise<void>;
    onRemove: () => Promise<void>;
    disabled?: boolean;
};

function KeyCapture(props: KeyCaptureProps) {
    const {onCapture} = props;
    const [capturing, setCapturing] = useState<boolean>(false);
    const keySelectRef = useRef<HTMLDivElement | null>(null);

    const isRemoveDisabled = props.disabled || props.label === null;

    const handleKeyDownEvent = useCallback(
        async (event: KeyboardEvent) => {
            event.preventDefault();

            // For some keys (e.g., the MediaPlayPause one), the code returned is empty and the event only contains a key.
            // Since we want to remain layout independent, we prefer to use the code value, but fall back to the key if required.
            let code = event.code || event.key;

            // Additionally, we need to check if the NumLock key is active, since the code returned by the event will always be the numpad digit,
            // however, in case it's deactivated, we want to bind the key instead (e.g., ArrowLeft instead of Numpad4).
            // The DOM_KEY_LOCATION defines the location of the key on the keyboard, where DOM_KEY_LOCATION_NUMPAD (value 3) corresponds to the numpad.
            if (
                event.location === KeyboardEvent.DOM_KEY_LOCATION_NUMPAD &&
                !event.getModifierState("NumLock")
            ) {
                code = event.key;
            }

            try {
                await onCapture(code);
            } finally {
                setCapturing(false);
            }
        },
        [onCapture],
    );

    const handleClickOutside = useCallback((event: MouseEvent) => {
        if (keySelectRef.current === null || keySelectRef.current.contains(event.target as Node))
            return;
        setCapturing(false);
    }, []);

    const handleKeySelectOnClick = async () => {
        if (props.disabled) return;

        void invokeSafe("audio_play_ui_click");

        setCapturing(!capturing);
    };

    const handleOnRemoveClick = async () => {
        if (isRemoveDisabled) return;

        void invokeSafe("audio_play_ui_click");

        if (capturing) {
            setCapturing(false);
            return;
        }

        await props.onRemove();
    };

    useEffect(() => {
        if (!capturing) return;

        document.addEventListener("keydown", handleKeyDownEvent);
        document.addEventListener("keyup", preventKeyUpEvent);
        document.addEventListener("click", handleClickOutside);

        // Poll for joystick button in parallel
        let cancelled = false;
        invokeSafe<string | null>("keybinds_capture_joystick_button").then(async code => {
            if (cancelled || code == null) return;
            try {
                await onCapture(code);
            } finally {
                setCapturing(false);
            }
        });

        return () => {
            cancelled = true; // command will timeout on its own after 10s
            document.removeEventListener("keydown", handleKeyDownEvent);
            document.removeEventListener("keyup", preventKeyUpEvent);
            document.removeEventListener("click", handleClickOutside);
        };
    }, [capturing, handleKeyDownEvent, handleClickOutside]);

    return (
        <div className="grow h-full min-w-0 flex flex-row items-center justify-center">
            <div
                ref={keySelectRef}
                onClick={handleKeySelectOnClick}
                className={clsx(
                    "w-full h-full min-w-10 min-h-8 grow text-sm py-1 px-2 rounded text-center flex items-center justify-center",
                    "bg-gray-300 border-2",
                    capturing
                        ? "border-r-gray-100 border-b-gray-100 border-t-gray-700 border-l-gray-700 [&>*]:translate-y-[1px] [&>*]:translate-x-[1px]"
                        : "border-t-gray-100 border-l-gray-100 border-r-gray-700 border-b-gray-700",
                    props.disabled ? "brightness-90 cursor-not-allowed" : "cursor-pointer",
                )}
            >
                <p className="truncate max-w-full">
                    {capturing ? "Press your key" : (props.label ?? "Not bound")}
                </p>
            </div>
            <svg
                onClick={handleOnRemoveClick}
                xmlns="http://www.w3.org/2000/svg"
                width="27"
                height="27"
                viewBox="0 0 24 24"
                fill="none"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                className={clsx(
                    "shrink-0 p-1 pr-0!",
                    isRemoveDisabled
                        ? "stroke-gray-500 cursor-not-allowed"
                        : "stroke-gray-700 hover:stroke-red-500 transition-colors cursor-pointer",
                )}
            >
                <path d="M18 6 6 18" />
                <path d="m6 6 12 12" />
            </svg>
        </div>
    );
}

const preventKeyUpEvent = (event: KeyboardEvent) => {
    event.preventDefault();
};

export default KeyCapture;
