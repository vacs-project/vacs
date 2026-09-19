import {clsx} from "clsx";
import {useEffect, useState} from "preact/hooks";
import {invokeSafe, invokeStrict} from "../../error.ts";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {useSettingsStore} from "../../stores/settings-store.ts";
import {isTauri} from "../../transport";
import {RingSound, RingSounds, RingSoundType} from "../../types/audio.ts";
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

    return (
        <div className="w-full flex flex-col gap-2 pt-2 border-t-2 border-zinc-200">
            <p className="font-semibold uppercase text-center">Ring sounds</p>
            <div className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 items-center">
                <p>Ring</p>
                {ringSounds !== undefined ? (
                    <RingSoundField
                        ringType="ring"
                        sound={ringSounds.ring}
                        applyRingSound={applyRingSound}
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
                    />
                ) : (
                    <p>Loading...</p>
                )}
            </div>
            <p className="text-sm text-gray-700">
                Pick a custom chime above (WAV, up to 30 s). Clear the selection to reset to the
                built-in chime.
            </p>
        </div>
    );
}

type RingSoundFieldProps = {
    ringType: RingSoundType;
    sound?: RingSound;
    disabled?: boolean;
    disabledReason?: string;
    applyRingSound: (ringType: RingSoundType, path: string | null) => Promise<void>;
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
        />
    );
}

export default RingSoundSettings;
