import Select from "../ui/Select.tsx";
import {Dispatch, StateUpdater, useEffect, useState} from "preact/hooks";
import {
    withTransmitLabels,
    isTransmitMode,
    TransmitConfig,
    TransmitConfigWithLabels,
    RadioConfig,
    withRadioLabels,
    RadioConfigWithLabels,
    isRadioIntegration,
} from "../../types/transmit.ts";
import {invokeSafe, invokeStrict} from "../../error.ts";
import KeyCapture from "./KeyCapture.tsx";
import {useCapabilitiesStore} from "../../stores/capabilities-store.ts";
import {clsx} from "clsx";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {TargetedEvent} from "preact";
import {RadioState} from "../../types/radio.ts";
import {useRadioState} from "../../hooks/radio-state-hook.ts";
import {transmitModeToKeybind} from "../../types/keybinds.ts";
import StatusIndicator, {Status} from "../ui/StatusIndicator.tsx";

function TransmitModeSettings() {
    const capKeybindListener = useCapabilitiesStore(state => state.keybindListener);
    const capPlatform = useCapabilitiesStore(state => state.platform);
    const [transmitConfig, setTransmitConfig] = useState<TransmitConfigWithLabels | undefined>(
        undefined,
    );
    const [radioConfig, setRadioConfig] = useState<RadioConfigWithLabels | undefined>(undefined);

    useEffect(() => {
        const fetchConfig = async () => {
            const transmitConfig = await invokeSafe<TransmitConfig>("keybinds_get_transmit_config");
            if (transmitConfig === undefined) return;
            const radioConfig = await invokeSafe<RadioConfig>("keybinds_get_radio_config");
            if (radioConfig === undefined) return;

            setTransmitConfig(await withTransmitLabels(transmitConfig));
            setRadioConfig(await withRadioLabels(radioConfig));
        };

        if (capKeybindListener) {
            void fetchConfig();
        }
    }, [capKeybindListener]);

    return (
        <div className="h-full flex flex-col">
            <div className="flex flex-col gap-0.5">
                <p className="w-full mb-2 text-center border-b-2 border-zinc-200 uppercase font-semibold">
                    Mode
                </p>
                {!capKeybindListener ? (
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
                                <TransmitConfigSettings
                                    transmitConfig={transmitConfig}
                                    setTransmitConfig={setTransmitConfig}
                                />
                            ) : (
                                <p className="w-full text-center">Loading...</p>
                            )}
                        </div>
                        <p className="py-2 px-3 text-sm text-gray-800">
                            <b>Voice activation:</b> Mic unmuted, toggle{" "}
                            <span className="bg-[#92e1fe] border-2 border-t-cyan-100 border-l-cyan-100 border-r-cyan-950 border-b-cyan-950 rounded px-1 text-xs text-black font-semibold">
                                RADIO PRIO
                            </span>{" "}
                            to mute.
                            <br />
                            <b>Push-to-talk:</b> Mic muted, press and hold key to talk in a call.
                            <br />
                            <b>Push-to-mute:</b> Mic unmuted, press and hold key to mute in a call.
                            <br />
                            <b>Radio Integration:</b> While not in a call, press and hold key to
                            transmit on radio. During a call, the key will behave as a PTT key.
                            Toggling{" "}
                            <span className="bg-[#92e1fe] border-2 border-t-cyan-100 border-l-cyan-100 border-r-cyan-950 border-b-cyan-950 rounded px-1 text-xs text-black font-semibold">
                                RADIO PRIO
                            </span>{" "}
                            forces radio transmission during a call.
                        </p>
                    </>
                )}
            </div>
            <div className="grow flex flex-col gap-0.5">
                <div className="w-full pt-1 mb-1 flex flex-row gap-2 items-center justify-center border-t-2 border-zinc-200">
                    <p className="font-semibold uppercase">Radio Integration</p>
                </div>
                {!capKeybindListener ? (
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
                        <div className="w-full px-3 flex flex-row gap-3 items-center justify-center">
                            {transmitConfig !== undefined && radioConfig !== undefined ? (
                                <RadioIntegrationSettings
                                    transmitConfig={transmitConfig}
                                    radioConfig={radioConfig}
                                    setRadioConfig={setRadioConfig}
                                />
                            ) : (
                                <p className="w-full text-center">Loading...</p>
                            )}
                        </div>
                        {radioConfig?.integration === "AudioForVatsim" ? (
                            <p className="py-2 px-3 text-sm text-gray-800 leading-4.5">
                                vacs simulates a key press for you to trigger a radio transmission
                                in AFV. <br />
                                Set this key as your PTT key in AFV. You will not press it yourself,
                                vacs will do so automatically for you. Choosing a rarely used key
                                such as ScrollLock helps avoid accidental triggers. <br />
                                <span className="font-semibold text-red-600">
                                    IMPORTANT:
                                </span> Do <b className="uppercase">not</b> set the key configured
                                in the &quot;Mode&quot; section above in AFV.
                            </p>
                        ) : radioConfig?.integration === "TrackAudio" ? (
                            <p className="py-2 px-3 text-sm text-gray-800 leading-4.5">
                                vacs can connect to your TrackAudio client to trigger transmissions
                                as well as monitor radio and frequency state.
                                <br />
                                <b>Note:</b> TrackAudio must be connected to VATSIM and tuned to at
                                least one frequency for the radio to show a successful connection.
                                <br />
                                Connection status is indicated by the button color:{" "}
                                <span className="bg-[#05cf9c] border-2 border-t-green-200 border-l-green-200 border-r-green-950 border-b-green-950 rounded px-1 text-xs text-black font-semibold">
                                    Radio
                                </span>{" "}
                                (idle and ready to receive),{" "}
                                <span className="bg-[#5B95F9] border-2 border-t-blue-300 border-l-blue-300 border-r-blue-900 border-b-blue-900 rounded px-1 text-xs text-black font-semibold">
                                    Radio
                                </span>{" "}
                                (receiving or transmitting), or{" "}
                                <span className="bg-red-500 border-2 border-t-red-200 border-l-red-200 border-r-red-900 border-b-red-900 rounded px-1 text-xs text-black font-semibold">
                                    Radio
                                </span>{" "}
                                (error). A gray button indicates the radio is not ready.
                            </p>
                        ) : (
                            <p className="py-2 px-3 text-sm text-gray-800 leading-4.5">
                                How did you get here?
                            </p>
                        )}
                    </>
                )}
            </div>
        </div>
    );
}

type TransmitConfigSettingsProps = {
    transmitConfig: TransmitConfigWithLabels;
    setTransmitConfig: Dispatch<StateUpdater<TransmitConfigWithLabels | undefined>>;
};

function TransmitConfigSettings({transmitConfig, setTransmitConfig}: TransmitConfigSettingsProps) {
    const capPlatform = useCapabilitiesStore(state => state.platform);
    const [waylandBinding, setWaylandBinding] = useState<string | undefined>(undefined);

    const handleOnTransmitCapture = async (code: string) => {
        if (transmitConfig === undefined || transmitConfig.mode === "VoiceActivation") return;

        let newConfig: TransmitConfig;
        switch (transmitConfig.mode) {
            case "PushToTalk":
                newConfig = {...transmitConfig, pushToTalk: code};
                break;
            case "PushToMute":
                newConfig = {...transmitConfig, pushToMute: code};
                break;
            case "RadioIntegration":
                newConfig = {...transmitConfig, radioPushToTalk: code};
                break;
        }

        try {
            await invokeStrict("keybinds_set_transmit_config", {transmitConfig: newConfig});
            setTransmitConfig(await withTransmitLabels(newConfig));
        } catch {}
    };

    const handleOnTransmitModeChange = async (value: string) => {
        if (!isTransmitMode(value) || transmitConfig === undefined) return;

        const previousTransmitConfig = transmitConfig;
        const newTransmitConfig = {...transmitConfig, mode: value};

        setTransmitConfig(newTransmitConfig);

        try {
            await invokeStrict("keybinds_set_transmit_config", {transmitConfig: newTransmitConfig});
        } catch {
            setTransmitConfig(previousTransmitConfig);
        }
    };

    const handleOnTransmitRemoveClick = async () => {
        if (transmitConfig === undefined || transmitConfig.mode === "VoiceActivation") return;

        let newConfig: TransmitConfig;
        switch (transmitConfig.mode) {
            case "PushToTalk":
                newConfig = {...transmitConfig, pushToTalk: null};
                break;
            case "PushToMute":
                newConfig = {...transmitConfig, pushToMute: null};
                break;
            case "RadioIntegration":
                newConfig = {...transmitConfig, radioPushToTalk: null};
                break;
        }

        try {
            await invokeStrict("keybinds_set_transmit_config", {transmitConfig: newConfig});
            setTransmitConfig(await withTransmitLabels(newConfig));
        } catch {}
    };

    const handleOpenSystemShortcutsOnClick = useAsyncDebounce(async () => {
        await invokeSafe("keybinds_open_system_shortcuts_settings");
    });

    useEffect(() => {
        const fetchExternalBinding = async () => {
            const keybind = transmitModeToKeybind(transmitConfig.mode);
            if (keybind === null) {
                setWaylandBinding(undefined);
                return;
            }

            const binding = await invokeSafe<string | null>("keybinds_get_external_binding", {
                keybind,
            });
            setWaylandBinding(binding ?? undefined);
        };

        if (capPlatform === "LinuxWayland" && transmitConfig !== undefined) {
            if (transmitConfig.mode === "VoiceActivation") {
                setWaylandBinding(undefined);
            } else {
                void fetchExternalBinding();
            }
        }
    }, [capPlatform, transmitConfig]);

    return (
        <>
            <Select
                className="w-[21ch]! h-full"
                name="keybind-mode"
                options={[
                    {value: "VoiceActivation", text: "Voice activation"},
                    {value: "PushToTalk", text: "Push-to-talk"},
                    {value: "PushToMute", text: "Push-to-mute"},
                    ...(capPlatform === "Windows" ||
                    capPlatform === "MacOs" ||
                    capPlatform === "LinuxWayland"
                        ? [{value: "RadioIntegration", text: "Radio Integration"}]
                        : []),
                ]}
                selected={transmitConfig.mode}
                onChange={handleOnTransmitModeChange}
            />
            {capPlatform === "LinuxWayland" ? (
                <div
                    onClick={handleOpenSystemShortcutsOnClick}
                    title={
                        transmitConfig.mode !== "VoiceActivation"
                            ? "On Wayland, shortcuts are managed by the system. Please configure the shortcut in your desktop environment settings. Click this field to try opening the appropriate system settings."
                            : ""
                    }
                    className={clsx(
                        "w-full h-full min-w-0 min-h-8 grow text-sm py-1 px-2 rounded text-center flex items-center justify-center",
                        "bg-gray-300 border-2 border-t-gray-100 border-l-gray-100 border-r-gray-700 border-b-gray-700",
                        "brightness-90 cursor-help",
                        transmitConfig.mode === "VoiceActivation" &&
                            "brightness-90 cursor-not-allowed",
                    )}
                >
                    <p className="truncate max-w-full">
                        {transmitConfig.mode !== "VoiceActivation"
                            ? waylandBinding || "Not bound"
                            : ""}
                    </p>
                </div>
            ) : (
                <KeyCapture
                    label={
                        transmitConfig.mode === "PushToTalk"
                            ? transmitConfig.pushToTalkLabel
                            : transmitConfig.mode === "PushToMute"
                              ? transmitConfig.pushToMuteLabel
                              : transmitConfig.mode === "RadioIntegration"
                                ? transmitConfig.radioPushToTalkLabel
                                : ""
                    }
                    onCapture={handleOnTransmitCapture}
                    onRemove={handleOnTransmitRemoveClick}
                    disabled={transmitConfig.mode === "VoiceActivation"}
                />
            )}
        </>
    );
}

type RadioIntegrationSettingsProps = {
    transmitConfig: TransmitConfigWithLabels;
    radioConfig: RadioConfigWithLabels;
    setRadioConfig: Dispatch<StateUpdater<RadioConfigWithLabels | undefined>>;
};

function RadioIntegrationSettings({
    transmitConfig,
    radioConfig,
    setRadioConfig,
}: RadioIntegrationSettingsProps) {
    const capKeybindEmitter = useCapabilitiesStore(state => state.keybindEmitter);
    const [trackAudioEndpoint, setTrackAudioEndpoint] = useState<string>(
        radioConfig.trackAudio?.endpoint ?? "",
    );

    const handleOnRadioIntegrationCapture = async (code: string) => {
        if (
            transmitConfig === undefined ||
            transmitConfig.mode !== "RadioIntegration" ||
            radioConfig === undefined
        ) {
            return;
        }

        let newConfig: RadioConfig;
        switch (radioConfig.integration) {
            case "AudioForVatsim":
                newConfig = {
                    ...radioConfig,
                    audioForVatsim: {
                        emit: code,
                    },
                };
                break;
            default:
                return;
        }

        try {
            await invokeStrict("keybinds_set_radio_config", {radioConfig: newConfig});
            setRadioConfig(await withRadioLabels(newConfig));
        } catch {}
    };

    const handleOnRadioIntegrationChange = async (value: string) => {
        if (!isRadioIntegration(value) || radioConfig === undefined) return;

        const previousRadioConfig = radioConfig;
        const newRadioConfig = {...radioConfig, integration: value};

        setRadioConfig(newRadioConfig);

        try {
            await invokeStrict("keybinds_set_radio_config", {radioConfig: newRadioConfig});
        } catch {
            setRadioConfig(previousRadioConfig);
        }
    };

    const handleOnRadioIntegrationRemoveClick = async () => {
        if (radioConfig === undefined) return;

        let newConfig: RadioConfig;
        switch (radioConfig.integration) {
            case "AudioForVatsim":
                newConfig = {
                    ...radioConfig,
                    audioForVatsim: {
                        emit: null,
                    },
                };
                break;
            default:
                return;
        }

        try {
            await invokeStrict("keybinds_set_radio_config", {radioConfig: newConfig});
            setRadioConfig(await withRadioLabels(newConfig));
        } catch {}
    };

    const handleOnTrackAudioEndpointChange = (e: TargetedEvent<HTMLInputElement>) => {
        if (!(e.target instanceof HTMLInputElement)) return;
        setTrackAudioEndpoint(e.target.value);
    };

    const handleOnTrackAudioEndpointCommit = async () => {
        if (
            transmitConfig === undefined ||
            transmitConfig.mode !== "RadioIntegration" ||
            radioConfig === undefined
        ) {
            return;
        }

        const endpoint = trackAudioEndpoint.trim() === "" ? null : trackAudioEndpoint.trim();
        if (endpoint === radioConfig.trackAudio?.endpoint) return;

        let newConfig: RadioConfig;
        if (radioConfig.integration === "TrackAudio") {
            newConfig = {
                ...radioConfig,
                trackAudio: {
                    endpoint: endpoint,
                },
            };
            try {
                await invokeStrict("keybinds_set_radio_config", {radioConfig: newConfig});
                setRadioConfig(await withRadioLabels(newConfig));
            } catch {
                setTrackAudioEndpoint(radioConfig.trackAudio?.endpoint ?? "");
            }
        }
    };

    return (
        <>
            <Select
                className="shrink-0 w-[21ch]! h-full"
                name="radio-integration"
                options={[
                    ...(capKeybindEmitter
                        ? [{value: "AudioForVatsim", text: "Audio for Vatsim"}]
                        : []),
                    {value: "TrackAudio", text: "TrackAudio"},
                ]}
                selected={radioConfig.integration}
                onChange={handleOnRadioIntegrationChange}
                disabled={transmitConfig.mode !== "RadioIntegration"}
            />
            {radioConfig.integration === "TrackAudio" ? (
                <div className="w-full flex flex-row gap-2 items-center">
                    <input
                        type="text"
                        className={clsx(
                            "w-full h-full px-3 py-1.5 border border-gray-700 bg-gray-300 rounded text-sm text-center focus:border-blue-500 focus:outline-none placeholder:text-gray-500",
                            "disabled:brightness-90 disabled:cursor-not-allowed",
                        )}
                        placeholder="localhost:49080"
                        title="The address where TrackAudio is running. Accepts a hostname or IP address, with an optional port (e.g., '192.168.1.69' or '192.168.1.69:49080'). If you're running TrackAudio on the same machine as vacs, you can leave this value empty as it will automatically attempt to connect to TrackAudio on its default listener at 'localhost:49080'."
                        value={trackAudioEndpoint}
                        onInput={handleOnTrackAudioEndpointChange}
                        onBlur={handleOnTrackAudioEndpointCommit}
                        onKeyDown={e => {
                            if (e.key === "Enter") {
                                e.currentTarget.blur();
                            }
                        }}
                        disabled={transmitConfig.mode !== "RadioIntegration"}
                    />
                    <TrackAudioStatusIndicator />
                </div>
            ) : (
                <KeyCapture
                    label={radioConfig.audioForVatsim?.emitLabel ?? null}
                    onCapture={handleOnRadioIntegrationCapture}
                    onRemove={handleOnRadioIntegrationRemoveClick}
                    disabled={transmitConfig.mode !== "RadioIntegration"}
                />
            )}
        </>
    );
}

const RadioStateAsIndicatorState: {[key in RadioState]: Status} = {
    NotConfigured: "gray",
    Disconnected: "red",
    Error: "red",
    Connected: "green",
    VoiceConnected: "green",
    RxIdle: "green",
    RxActive: "green",
    TxActive: "green",
};

function TrackAudioStatusIndicator() {
    const {state, canReconnect, handleButtonClick} = useRadioState();

    const title = canReconnect
        ? "Reconnect to TrackAudio"
        : state !== "NotConfigured"
          ? "Connected to TrackAudio"
          : "Deactivated";

    return (
        <StatusIndicator
            status={RadioStateAsIndicatorState[state]}
            className={canReconnect ? "cursor-pointer" : undefined}
            onClick={handleButtonClick}
            title={title}
        />
    );
}

export default TransmitModeSettings;
