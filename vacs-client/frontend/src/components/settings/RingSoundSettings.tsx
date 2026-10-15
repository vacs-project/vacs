import {clsx} from "clsx";
import {useEffect, useState} from "preact/hooks";
import {invokeSafe, invokeStrict} from "../../error.ts";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {useSettingsStore} from "../../stores/settings-store.ts";
import {isTauri} from "../../transport";
import {RingSound, RingSounds, RingSoundType} from "../../types/audio.ts";
import Hint from "../Hint.tsx";
import SelectionField from "../ui/SelectionField.tsx";

function fileName(path: string): string {
    return path.split(/[\\/]/).pop() || path;
}

function RingSoundSettings() {
    const [ringSounds, setRingSounds] = useState<RingSounds | undefined>(undefined);
    const priorityCallsEnabled = useSettingsStore(state => state.callConfig.enablePriorityCalls);

    useEffect(() => {
        const fetchRingSounds = async () => {
            const sounds = await invokeSafe<RingSounds>("audio_get_ring_sounds");
            setRingSounds(sounds ?? {});
        };

        void fetchRingSounds();
    }, []);

    const applyRingSound = async (ringType: RingSoundType, path: string | null) => {
        try {
            const sounds = await invokeStrict<RingSounds>("audio_set_ring_sound", {
                ringType,
                path,
            });
            setRingSounds(sounds);
        } catch {}
    };

    const applyRingSoundLoop = async (ringType: RingSoundType, enabled: boolean) => {
        try {
            const sounds = await invokeStrict<RingSounds>("audio_set_ring_sound_loop", {
                ringType,
                enabled,
            });
            setRingSounds(sounds);
        } catch {}
    };

    return (
        <div className="w-full flex flex-col gap-2 pt-1 pb-3 border-t-2 border-zinc-200">
            <div className="flex flex-row gap-2 items-center justify-center">
                <p className="font-semibold uppercase text-center">Ring sounds</p>
                <Hint>
                    <p>
                        Pick a custom chime (WAV, up to 30s). Clear the selection to reset to the
                        built-in chime. The loop button repeats a custom chime until the call is
                        answered or ends.
                    </p>
                </Hint>
            </div>
            <div className="px-4 grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 items-center">
                <p>Ring</p>
                {ringSounds !== undefined ? (
                    <RingSoundField
                        ringType="ring"
                        sound={ringSounds.ring}
                        applyRingSound={applyRingSound}
                        applyRingSoundLoop={applyRingSoundLoop}
                    />
                ) : (
                    <p>Loading...</p>
                )}
                <p>Priority ring</p>
                {ringSounds !== undefined ? (
                    <RingSoundField
                        ringType="priorityRing"
                        sound={ringSounds.priorityRing}
                        disabled={!priorityCallsEnabled}
                        disabledReason="Enable priority calls to use a priority ring"
                        applyRingSound={applyRingSound}
                        applyRingSoundLoop={applyRingSoundLoop}
                    />
                ) : (
                    <p>Loading...</p>
                )}
            </div>
        </div>
    );
}

type RingSoundFieldProps = {
    ringType: RingSoundType;
    sound?: RingSound;
    disabled?: boolean;
    disabledReason?: string;
    applyRingSound: (ringType: RingSoundType, path: string | null) => Promise<void>;
    applyRingSoundLoop: (ringType: RingSoundType, enabled: boolean) => Promise<void>;
};

function RingSoundField(props: RingSoundFieldProps) {
    const disabled = props.disabled === true || !isTauri;
    const unavailable = props.sound !== undefined && !props.sound.available;

    const handleOnClick = useAsyncDebounce(async () => {
        try {
            const path = await invokeStrict<string | null>("audio_pick_ring_sound");
            if (path === null || path === undefined) return;
            await props.applyRingSound(props.ringType, path);
        } catch {}
    });

    const handleOnRemove = useAsyncDebounce(async () => {
        await props.applyRingSound(props.ringType, null);
    });

    const handleOnLoopToggle = useAsyncDebounce(async () => {
        if (props.sound === undefined) return;
        await props.applyRingSoundLoop(props.ringType, !props.sound.loop);
    });

    const title = [
        unavailable
            ? `${props.sound?.path}: the file could not be loaded, the built-in chime plays`
            : (props.sound?.path ?? (disabled ? undefined : "Click to choose a WAV file")),
        props.disabled ? props.disabledReason : undefined,
        isTauri ? undefined : "Choosing a file is only possible in the desktop client",
    ]
        .filter(part => part !== undefined)
        .join("\n");

    return (
        <SelectionField
            label={props.sound === undefined ? "Built-in chime" : fileName(props.sound.path)}
            title={title}
            disabled={disabled}
            removeDisabled={props.disabled === true || props.sound === undefined}
            className={clsx(
                props.sound === undefined && "text-gray-500",
                unavailable && "text-red-700",
            )}
            onClick={handleOnClick}
            onRemove={handleOnRemove}
            actions={
                <LoopToggle
                    enabled={props.sound?.loop === true}
                    disabled={props.disabled === true || props.sound?.available !== true}
                    disabledReason={
                        props.disabled ? props.disabledReason : "Only a custom chime can repeat"
                    }
                    onToggle={handleOnLoopToggle}
                />
            }
        />
    );
}

type LoopToggleProps = {
    enabled: boolean;
    disabled: boolean;
    disabledReason?: string;
    onToggle: () => void;
};

function LoopToggle(props: LoopToggleProps) {
    const handleOnClick = () => {
        if (props.disabled) return;
        void invokeSafe("audio_play_ui_click");
        props.onToggle();
    };

    const title = props.disabled
        ? props.disabledReason
        : props.enabled
          ? "Repeats until the call is answered. Click to play once."
          : "Plays once. Click to repeat until the call is answered.";

    return (
        <svg
            onClick={handleOnClick}
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
                props.disabled
                    ? "stroke-gray-500 cursor-not-allowed"
                    : props.enabled
                      ? "stroke-blue-700 cursor-pointer"
                      : "stroke-gray-700 hover:stroke-blue-700 transition-colors cursor-pointer",
            )}
        >
            <title>{title}</title>
            <path d="m17 2 4 4-4 4" />
            <path d="M3 11v-1a4 4 0 0 1 4-4h14" />
            <path d="m7 22-4-4 4-4" />
            <path d="M21 13v1a4 4 0 0 1-4 4H3" />
        </svg>
    );
}

export default RingSoundSettings;
