import KeyCapture from "./KeyCapture.tsx";
import {codeToLabel} from "../../types/transmit.ts";
import {useEffect, useState} from "preact/hooks";
import {KeybindsConfig, KeybindType} from "../../types/keybinds.ts";
import {invokeSafe, invokeStrict} from "../../error.ts";
import {useCapabilitiesStore} from "../../stores/capabilities-store.ts";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {clsx} from "clsx";
import SettingsSubPage from "./SettingsSubPage.tsx";

type Keybind = {
    code: string | null;
    label: string | null;
};

async function codeToKeybind(code: string | null): Promise<Keybind> {
    return {code, label: code && (await codeToLabel(code))};
}

function HotkeysConfigPage() {
    const [acceptCall, setAcceptCall] = useState<Keybind | undefined>(undefined);
    const [endCall, setEndCall] = useState<Keybind | undefined>(undefined);
    const [toggleRadioPrio, setToggleRadioPrio] = useState<Keybind | undefined>(undefined);

    useEffect(() => {
        const fetchConfig = async () => {
            try {
                const config = await invokeStrict<KeybindsConfig>("keybinds_get_keybinds_config");
                setAcceptCall(await codeToKeybind(config.acceptCall));
                setEndCall(await codeToKeybind(config.endCall));
                setToggleRadioPrio(await codeToKeybind(config.toggleRadioPrio));
            } catch {}
        };

        void fetchConfig();
    }, []);

    return (
        <SettingsSubPage title="Hotkeys Config" width="w-1/2" className="py-3 px-4">
            <div className="grid grid-cols-[auto_1fr] gap-4 items-center">
                <KeybindField
                    type="AcceptCall"
                    label="Accept first call"
                    keybind={acceptCall}
                    setKeybind={setAcceptCall}
                />
                <KeybindField
                    type="EndCall"
                    label="End active call"
                    keybind={endCall}
                    setKeybind={setEndCall}
                />
                <KeybindField
                    type="ToggleRadioPrio"
                    label="Toggle RADIO PRIO"
                    keybind={toggleRadioPrio}
                    setKeybind={setToggleRadioPrio}
                />
            </div>
        </SettingsSubPage>
    );
}

type KeybindFieldProps = {
    type: KeybindType;
    label: string;
    keybind?: Keybind;
    setKeybind: (keybind: Keybind) => void;
};

function KeybindField({type, label, keybind, setKeybind}: KeybindFieldProps) {
    const hasExternal = useCapabilitiesStore(state => state.platform === "LinuxWayland");

    const handleOnCapture = async (code: string | null) => {
        try {
            await invokeStrict("keybinds_set_binding", {keybind: type, code});
            setKeybind(await codeToKeybind(code));
        } catch {}
    };

    return (
        <>
            <p>{label}</p>
            {hasExternal ? (
                <ExternalKeybindField type={type} />
            ) : keybind !== undefined ? (
                <KeyCapture
                    label={keybind.label}
                    onCapture={handleOnCapture}
                    onRemove={() => handleOnCapture(null)}
                />
            ) : (
                <p>Loading...</p>
            )}
        </>
    );
}

function ExternalKeybindField({type}: {type: KeybindType}) {
    const [binding, setBinding] = useState<string | null | undefined>(undefined);

    const handleOpenSystemShortcutsOnClick = useAsyncDebounce(async () => {
        await invokeSafe("keybinds_open_system_shortcuts_settings");
    });

    useEffect(() => {
        const fetchExternalBinding = async () => {
            try {
                const binding = await invokeStrict<string | null>("keybinds_get_external_binding", {
                    keybind: type,
                });
                setBinding(binding);
            } catch {}
        };

        void fetchExternalBinding();
    }, [type]);

    return (
        <div
            onClick={handleOpenSystemShortcutsOnClick}
            title="On Wayland, shortcuts are managed by the system. Please configure the shortcut in your desktop environment settings. Click this field to try opening the appropriate system settings."
            className={clsx(
                "w-full h-full min-w-10 min-h-8 grow text-sm py-1 px-2 rounded text-center flex items-center justify-center",
                "bg-gray-300 border-2 border-t-gray-100 border-l-gray-100 border-r-gray-700 border-b-gray-700",
                "brightness-90 cursor-help",
            )}
        >
            <p className="truncate max-w-full">{binding || "Not bound"}</p>
        </div>
    );
}

export default HotkeysConfigPage;
