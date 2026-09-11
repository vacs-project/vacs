import KeyCapture from "./KeyCapture.tsx";
import {InputBinding, inputToLabel} from "../../types/transmit.ts";
import {useEffect, useState} from "preact/hooks";
import {KeybindsConfig, KeybindType} from "../../types/keybinds.ts";
import {invokeStrict} from "../../error.ts";
import {selectPortalShortcuts, useCapabilitiesStore} from "../../stores/capabilities-store.ts";
import SettingsSubPage from "./SettingsSubPage.tsx";
import ExternalKeybindField from "./ExternalKeybindField.tsx";
import KeybindPageActions from "./KeybindPageActions.tsx";

type Keybind = {
    input: InputBinding | null;
    label: string | null;
};

async function inputToKeybind(input: InputBinding | null): Promise<Keybind> {
    return {input, label: input && (await inputToLabel(input))};
}

function HotkeysConfigPage() {
    const capPlatform = useCapabilitiesStore(state => state.platform);
    const capKeybindListener = useCapabilitiesStore(state => state.keybindListener);
    const [acceptCall, setAcceptCall] = useState<Keybind | undefined>(undefined);
    const [endCall, setEndCall] = useState<Keybind | undefined>(undefined);
    const [toggleRadioPrio, setToggleRadioPrio] = useState<Keybind | undefined>(undefined);

    useEffect(() => {
        const fetchConfig = async () => {
            try {
                const config = await invokeStrict<KeybindsConfig>("keybinds_get_keybinds_config");
                setAcceptCall(await inputToKeybind(config.acceptCall));
                setEndCall(await inputToKeybind(config.endCall));
                setToggleRadioPrio(await inputToKeybind(config.toggleRadioPrio));
            } catch {}
        };

        void fetchConfig();
    }, []);

    return (
        <SettingsSubPage
            title="Hotkeys Config"
            width="w-1/2"
            actions={<KeybindPageActions />}
            className="py-3 px-4"
        >
            {capPlatform === "LinuxWayland" && !capKeybindListener && (
                <p className="pb-3 text-sm text-gray-700">
                    Keyboard shortcuts are unavailable on this desktop: its portal offers no global
                    shortcuts. Joystick buttons can still be bound.
                </p>
            )}
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
    const hasExternal = useCapabilitiesStore(selectPortalShortcuts);

    const handleOnCapture = async (input: InputBinding | null) => {
        try {
            await invokeStrict("keybinds_set_binding", {keybind: type, input});
            setKeybind(await inputToKeybind(input));
        } catch {}
    };

    return (
        <>
            <p>{label}</p>
            {hasExternal ? (
                <ExternalKeybindField
                    type={type}
                    binding={keybind?.input ?? null}
                    bindingLabel={keybind?.label ?? null}
                    onCapture={handleOnCapture}
                    onRemove={() => handleOnCapture(null)}
                />
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

export default HotkeysConfigPage;
