import {useCallback, useEffect, useRef, useState} from "preact/hooks";
import {invokeSafe, invokeStrict} from "../../error.ts";
import {useCapabilitiesStore} from "../../stores/capabilities-store.ts";
import {InputBinding, JoystickButton} from "../../types/transmit.ts";
import SelectionField from "../ui/SelectionField.tsx";

type KeyCaptureProps = {
    label: string | null;
    className?: string;
    onCapture: (input: InputBinding) => Promise<void>;
    onRemove: () => Promise<void>;
    disabled?: boolean;
    /// Disable the remove button even when a label is shown (e.g. when the
    /// label displays an externally managed binding that cannot be removed).
    removeDisabled?: boolean;
    /// Capture keyboard keys. Defaults to the keybindListener capability, so
    /// platforms without global keyboard capture (X11) get joystick-only fields.
    keyboardEnabled?: boolean;
    /// Capture joystick buttons (additionally requires the joystick capability).
    joystickEnabled?: boolean;
};

// On Windows, pressing the physical AltGr/right-Alt key makes the browser fire a synthetic
// ControlLeft keydown immediately before the real AltRight keydown. We only hold back a
// ControlLeft capture by this long to let a following AltRight supersede it as the ghost
// event; every other key still commits immediately.
const CONTROL_LEFT_DEBOUNCE_MS = 50;

function KeyCapture(props: KeyCaptureProps) {
    const {onCapture} = props;
    const capKeybindListener = useCapabilitiesStore(state => state.keybindListener);
    const capJoystick = useCapabilitiesStore(state => state.joystick);
    const [capturing, setCapturing] = useState<boolean>(false);
    const keySelectRef = useRef<HTMLDivElement | null>(null);
    const captureDebounceTimerRef = useRef<number | undefined>(undefined);
    // Incremented on every capture start/stop so a joystick capture resolving
    // after the capture UI closed (or a newer capture started) is ignored.
    const captureGenerationRef = useRef<number>(0);

    const keyboardEnabled = props.keyboardEnabled ?? capKeybindListener;
    const joystickEnabled = (props.joystickEnabled ?? true) && capJoystick;

    const isRemoveDisabled = props.disabled || props.removeDisabled || props.label === null;

    const commitCapture = useCallback(
        async (input: InputBinding) => {
            captureDebounceTimerRef.current = undefined;
            captureGenerationRef.current++;
            try {
                await onCapture(input);
            } finally {
                setCapturing(false);
            }
        },
        [onCapture],
    );

    const handleKeyDownEvent = useCallback(
        (event: KeyboardEvent) => {
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

            window.clearTimeout(captureDebounceTimerRef.current);

            if (code === "ControlLeft") {
                captureDebounceTimerRef.current = window.setTimeout(() => {
                    void commitCapture(code);
                }, CONTROL_LEFT_DEBOUNCE_MS);
                return;
            }

            void commitCapture(code);
        },
        [commitCapture],
    );

    const handleClickOutside = useCallback((event: MouseEvent) => {
        if (keySelectRef.current === null || keySelectRef.current.contains(event.target as Node))
            return;
        setCapturing(false);
    }, []);

    const handleKeySelectOnClick = () => {
        setCapturing(!capturing);
    };

    const handleOnRemoveClick = async () => {
        if (capturing) {
            setCapturing(false);
            return;
        }

        await props.onRemove();
    };

    useEffect(() => {
        if (!capturing) return;

        const generation = ++captureGenerationRef.current;
        const captureId = crypto.randomUUID();
        // Cleared by the cleanup so a joystick capture resolving after this
        // session ended is dropped; the generation guards the same within an
        // active session, where a keyboard key may commit first.
        let active = true;

        if (keyboardEnabled) {
            document.addEventListener("keydown", handleKeyDownEvent);
            document.addEventListener("keyup", preventKeyUpEvent);
        }
        document.addEventListener("click", handleClickOutside);

        if (joystickEnabled) {
            // The webview cannot observe joystick input (and would not receive it
            // while unfocused anyway), so the backend captures the next pressed
            // button for us. Resolves with null on timeout or cancellation; on
            // timeout we re-arm as long as this capture session is still active.
            const captureLoop = async () => {
                try {
                    const button = await invokeStrict<JoystickButton | null>(
                        "keybinds_capture_joystick_button",
                        {captureId},
                    );

                    if (!active || captureGenerationRef.current !== generation) return;

                    if (button === null) {
                        void captureLoop();
                        return;
                    }

                    void commitCapture(button);
                } catch {}
            };
            void captureLoop();
        }

        return () => {
            if (capturing) {
                active = false;
                if (keyboardEnabled) {
                    document.removeEventListener("keydown", handleKeyDownEvent);
                    document.removeEventListener("keyup", preventKeyUpEvent);
                }
                document.removeEventListener("click", handleClickOutside);
                window.clearTimeout(captureDebounceTimerRef.current);
                captureDebounceTimerRef.current = undefined;
                if (joystickEnabled) {
                    void invokeSafe("keybinds_cancel_joystick_capture", {captureId});
                }
            }
        };
    }, [
        capturing,
        keyboardEnabled,
        joystickEnabled,
        handleKeyDownEvent,
        handleClickOutside,
        commitCapture,
    ]);

    const capturePrompt =
        keyboardEnabled && joystickEnabled
            ? "Press key or button"
            : joystickEnabled
              ? "Press a button"
              : "Press your key";

    return (
        <SelectionField
            ref={keySelectRef}
            label={capturing ? capturePrompt : (props.label ?? "Not bound")}
            title={props.label ?? undefined}
            active={capturing}
            disabled={props.disabled}
            removeDisabled={isRemoveDisabled}
            className={props.className}
            onClick={handleKeySelectOnClick}
            onRemove={handleOnRemoveClick}
        />
    );
}

const preventKeyUpEvent = (event: KeyboardEvent) => {
    event.preventDefault();
};

export default KeyCapture;
