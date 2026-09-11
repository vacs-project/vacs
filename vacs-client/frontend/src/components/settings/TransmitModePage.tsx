import {useCapabilitiesStore} from "../../stores/capabilities-store.ts";
import {useSettingsStore} from "../../stores/settings-store.ts";
import HelpIcon from "../ui/HelpIcon.tsx";
import RadioPrioBadge from "../ui/RadioPrioBadge.tsx";
import CallMicModeSettings from "./CallMicModeSettings.tsx";
import KeybindPageActions from "./KeybindPageActions.tsx";
import RadioIntegrationSettings from "./RadioIntegrationSettings.tsx";
import SettingsSubPage from "./SettingsSubPage.tsx";

function TransmitModePage() {
    const capKeybindListener = useCapabilitiesStore(state => state.keybindListener);
    const capJoystick = useCapabilitiesStore(state => state.joystick);
    const capPlatform = useCapabilitiesStore(state => state.platform);
    // Without a keyboard listener (e.g. X11), joystick buttons can still be bound.
    const capAnyInput = capKeybindListener || capJoystick;

    const transmitConfig = useSettingsStore(state => state.transmitConfig);
    const setTransmitConfig = useSettingsStore(state => state.setTransmitConfig);
    const radioConfig = useSettingsStore(state => state.radioConfig);
    const setRadioConfig = useSettingsStore(state => state.setRadioConfig);

    return (
        <SettingsSubPage
            title="Transmit Config"
            width="w-[69%]"
            actions={<KeybindPageActions />}
            className="flex flex-col overflow-y-auto"
        >
            <div className="h-full flex flex-col">
                <div className="flex flex-col gap-0.5">
                    <div className="w-full mb-1 flex flex-row gap-2 items-center justify-center border-b-2 border-zinc-200">
                        <p className="font-semibold uppercase">Call Mic Mode</p>
                        <HelpIcon url="https://docs.vacs.network/settings/transmit#call-mic-mode" />
                    </div>
                    {capPlatform === "LinuxWayland" && !capKeybindListener && (
                        <p className="px-3 py-1 text-sm text-gray-700">
                            Keyboard shortcuts are unavailable on this desktop: its portal offers no
                            global shortcuts. Joystick buttons can still be bound.
                        </p>
                    )}
                    {!capAnyInput ? (
                        <div className="w-full px-3 flex flex-row gap-3 items-center justify-center">
                            <p
                                className="text-sm text-gray-700 py-1.5 cursor-help"
                                title={`Unfortunately, keybinds are not yet supported on ${capPlatform}`}
                            >
                                Not available.
                            </p>
                        </div>
                    ) : (
                        <>
                            <div className="w-full px-3 flex flex-row gap-3 items-center justify-center">
                                {transmitConfig !== undefined ? (
                                    <CallMicModeSettings
                                        transmitConfig={transmitConfig}
                                        setTransmitConfig={setTransmitConfig}
                                    />
                                ) : (
                                    <p className="w-full text-center">Loading...</p>
                                )}
                            </div>
                            <p className="py-2 px-3 text-sm text-gray-800">
                                <b>Voice activation:</b> Mic unmuted, toggle <RadioPrioBadge /> to
                                mute.
                                <br />
                                <b>Push-to-talk:</b> Mic muted, press and hold key to talk in a
                                call.
                                <br />
                                <b>Push-to-mute:</b> Mic unmuted, press and hold key to mute in a
                                call.
                            </p>
                        </>
                    )}
                </div>
                <div className="grow flex flex-col gap-0.5">
                    <div className="w-full pt-1 mb-1 flex flex-row gap-2 items-center justify-center border-t-2 border-zinc-200">
                        <p className="font-semibold uppercase">Radio Integration</p>
                        <HelpIcon url="https://docs.vacs.network/settings/transmit#radio-integration" />
                    </div>
                    {!capAnyInput ? (
                        <div className="w-full px-3 flex flex-row gap-3 items-center justify-center">
                            <p
                                className="text-sm text-gray-700 py-1.5 cursor-help"
                                title={`Unfortunately, keybind emitters are not yet supported on ${capPlatform}`}
                            >
                                Not available.
                            </p>
                        </div>
                    ) : (
                        <>
                            {transmitConfig !== undefined && radioConfig !== undefined ? (
                                <RadioIntegrationSettings
                                    transmitConfig={transmitConfig}
                                    radioConfig={radioConfig}
                                    setTransmitConfig={setTransmitConfig}
                                    setRadioConfig={setRadioConfig}
                                />
                            ) : (
                                <div className="w-full px-3 flex items-center justify-center">
                                    <p className="w-full text-center">Loading...</p>
                                </div>
                            )}
                        </>
                    )}
                </div>
            </div>
        </SettingsSubPage>
    );
}

export default TransmitModePage;
