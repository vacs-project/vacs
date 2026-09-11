import {clsx} from "clsx";
import {TargetedEvent} from "preact";
import {useEffect, useState} from "preact/hooks";
import {invokeStrict} from "../../error.ts";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {selectPortalShortcuts, useCapabilitiesStore} from "../../stores/capabilities-store.ts";
import {setPage} from "../../stores/navigation-store.ts";
import {useRadioStore} from "../../stores/radio-store.ts";
import {callMicModeToKeybind, KeybindType} from "../../types/keybinds.ts";
import {RadioState} from "../../types/radio.ts";
import {
    CallMicMode,
    InputBinding,
    RadioConfig,
    RadioConfigWithLabels,
    TransmitConfig,
    TransmitConfigWithLabels,
    inputEquals,
    isRadioIntegration,
    withRadioLabels,
    withTransmitLabels,
} from "../../types/transmit.ts";
import RadioPrioBadge from "../ui/RadioPrioBadge.tsx";
import Select from "../ui/Select.tsx";
import StatusIndicator, {Status} from "../ui/StatusIndicator.tsx";
import ExternalKeybindField from "./ExternalKeybindField.tsx";
import KeyCapture from "./KeyCapture.tsx";

type RadioIntegrationSettingsProps = {
    transmitConfig: TransmitConfigWithLabels;
    radioConfig: RadioConfigWithLabels;
    setTransmitConfig: (config: TransmitConfigWithLabels) => void;
    setRadioConfig: (config: RadioConfigWithLabels) => void;
};

function RadioIntegrationSettings({
    transmitConfig,
    radioConfig,
    setTransmitConfig,
    setRadioConfig,
}: RadioIntegrationSettingsProps) {
    const usesPortalShortcuts = useCapabilitiesStore(selectPortalShortcuts);
    const capKeybindEmitter = useCapabilitiesStore(state => state.keybindEmitter);
    const [trackAudioEndpoint, setTrackAudioEndpoint] = useState<string>(
        radioConfig.trackAudio?.endpoint ?? "",
    );
    const [waylandRadioKeybindType, setWaylandRadioKeybindType] = useState<KeybindType | null>(
        null,
    );

    const handleOnRadioIntegrationChange = async (value: string) => {
        const previousRadioConfig = radioConfig;

        let newRadioConfig;
        if (isRadioIntegration(value)) {
            newRadioConfig = {...radioConfig, integration: value};
        } else if (value === "None") {
            newRadioConfig = {...radioConfig, integration: null};
        } else {
            return;
        }

        setRadioConfig(newRadioConfig);

        try {
            await invokeStrict("radio_set_config", {radioConfig: newRadioConfig});
            if (value === "AudioForVatsim") {
                setPage("phone");
            }
        } catch {
            setRadioConfig(previousRadioConfig);
        }
    };

    const handleOnRadioPushToTalkCapture = async (input: InputBinding) => {
        if (inputEquals(input, transmitConfig.radioPushToTalk)) return;

        let newConfig: TransmitConfig = {...transmitConfig, radioPushToTalk: input};

        if (transmitConfig.callMicMode !== "VoiceActivation") {
            const callKey =
                transmitConfig.callMicMode === "PushToTalk"
                    ? transmitConfig.pushToTalk
                    : transmitConfig.pushToMute;

            if (inputEquals(callKey, input)) {
                newConfig.radioPushToTalk = null;
            }
        }

        try {
            await invokeStrict("keybinds_set_transmit_config", {transmitConfig: newConfig});
            setTransmitConfig(await withTransmitLabels(newConfig));
        } catch {}
    };

    const handleOnRadioPushToTalkRemoveClick = async () => {
        const newConfig: TransmitConfig = {...transmitConfig, radioPushToTalk: null};

        try {
            await invokeStrict("keybinds_set_transmit_config", {transmitConfig: newConfig});
            setTransmitConfig(await withTransmitLabels(newConfig));
        } catch {}
    };

    const handleOnAfvEmitCapture = async (input: InputBinding) => {
        // The AFV emit key is injected into another application by the keybind
        // emitter, so it must be a keyboard key (joystick capture is disabled
        // on its KeyCapture field).
        if (radioConfig.integration !== "AudioForVatsim" || typeof input !== "string") return;

        const newConfig: RadioConfig = {
            ...radioConfig,
            audioForVatsim: {emit: input},
        };

        try {
            await invokeStrict("radio_set_config", {radioConfig: newConfig});
            setRadioConfig(await withRadioLabels(newConfig));
        } catch {}
    };

    const handleOnAfvEmitRemoveClick = async () => {
        if (radioConfig.integration !== "AudioForVatsim") return;

        const newConfig: RadioConfig = {
            ...radioConfig,
            audioForVatsim: {emit: null},
        };

        try {
            await invokeStrict("radio_set_config", {radioConfig: newConfig});
            setRadioConfig(await withRadioLabels(newConfig));
        } catch {}
    };

    const handleOnTrackAudioEndpointChange = (e: TargetedEvent<HTMLInputElement>) => {
        if (!(e.target instanceof HTMLInputElement)) return;
        setTrackAudioEndpoint(e.target.value);
    };

    const handleOnTrackAudioEndpointCommit = async () => {
        if (radioConfig.integration !== "TrackAudio") return;

        const endpoint = trackAudioEndpoint.trim() === "" ? null : trackAudioEndpoint.trim();
        if (endpoint === radioConfig.trackAudio?.endpoint) return;

        const newConfig: RadioConfig = {
            ...radioConfig,
            trackAudio: {endpoint},
        };

        try {
            await invokeStrict("radio_set_config", {radioConfig: newConfig});
            setRadioConfig(await withRadioLabels(newConfig));
        } catch {
            setTrackAudioEndpoint(radioConfig.trackAudio?.endpoint ?? "");
        }
    };

    useEffect(() => {
        const setWaylandRadioKeybind = async () => {
            if (radioConfig.integration === null) {
                setWaylandRadioKeybindType(null);
                return;
            } else if (transmitConfig.callMicMode === "PushToMute") {
                setWaylandRadioKeybindType("PushToMute");
                return;
            } else if (transmitConfig.callMicMode === "VoiceActivation") {
                setWaylandRadioKeybindType("RadioPushToTalk");
                return;
            }

            try {
                const isBound = await invokeStrict<boolean>("keybinds_is_portal_shortcut_bound", {
                    keybind: "RadioPushToTalk",
                });
                setWaylandRadioKeybindType(isBound ? "RadioPushToTalk" : "PushToTalk");
            } catch {}
        };

        if (usesPortalShortcuts) {
            void setWaylandRadioKeybind();
        }
    }, [transmitConfig.callMicMode, radioConfig.integration, usesPortalShortcuts]);

    return (
        <div className="w-full px-3 flex flex-col gap-2 items-center justify-center">
            <div className="w-full flex flex-row gap-3 items-center justify-center">
                <Select
                    className="shrink-0 w-[21ch]! h-full"
                    name="radio-integration"
                    options={[
                        {value: "None", text: "None"},
                        {value: "TrackAudio", text: "TrackAudio"},
                        ...(capKeybindEmitter
                            ? [{value: "AudioForVatsim", text: "Audio for Vatsim"}]
                            : []),
                    ]}
                    selected={radioConfig.integration ?? "None"}
                    onChange={handleOnRadioIntegrationChange}
                />
                {usesPortalShortcuts ? (
                    <ExternalKeybindField
                        type={waylandRadioKeybindType}
                        binding={
                            transmitConfig.callMicMode === "PushToTalk"
                                ? (transmitConfig.radioPushToTalk ?? transmitConfig.pushToTalk)
                                : transmitConfig.callMicMode === "VoiceActivation"
                                  ? transmitConfig.radioPushToTalk
                                  : transmitConfig.pushToMute
                        }
                        bindingLabel={
                            transmitConfig.callMicMode === "PushToTalk"
                                ? (transmitConfig.radioPushToTalkLabel ??
                                  transmitConfig.pushToTalkLabel)
                                : transmitConfig.callMicMode === "VoiceActivation"
                                  ? transmitConfig.radioPushToTalkLabel
                                  : transmitConfig.pushToMuteLabel
                        }
                        className={clsx(
                            callMicModeToKeybind(transmitConfig.callMicMode) ===
                                waylandRadioKeybindType &&
                                transmitConfig.radioPushToTalk === null &&
                                "text-gray-500",
                        )}
                        disabled={
                            radioConfig.integration === null ||
                            transmitConfig.callMicMode === "PushToMute"
                        }
                        onCapture={handleOnRadioPushToTalkCapture}
                        onRemove={handleOnRadioPushToTalkRemoveClick}
                    />
                ) : (
                    <KeyCapture
                        label={
                            radioConfig.integration === null
                                ? ""
                                : transmitConfig.callMicMode === "VoiceActivation"
                                  ? transmitConfig.radioPushToTalkLabel
                                  : transmitConfig.callMicMode === "PushToTalk"
                                    ? (transmitConfig.radioPushToTalkLabel ??
                                      transmitConfig.pushToTalkLabel)
                                    : transmitConfig.pushToMuteLabel
                        }
                        className={clsx(
                            transmitConfig.radioPushToTalkLabel === null && "text-gray-500",
                        )}
                        disabled={
                            radioConfig.integration === null ||
                            transmitConfig.callMicMode === "PushToMute"
                        }
                        onCapture={handleOnRadioPushToTalkCapture}
                        onRemove={handleOnRadioPushToTalkRemoveClick}
                    />
                )}
            </div>
            {radioConfig.integration === "TrackAudio" ? (
                <>
                    <RadioPttDescription callMicMode={transmitConfig.callMicMode} />
                    <p className="text-sm text-gray-800 leading-4.5">
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
                    <div className="w-full flex flex-row items-center">
                        <div className="w-full flex flex-row gap-3 items-center justify-center">
                            <p className="text-sm w-[21ch]! shrink-0 text-right">Endpoint:</p>
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
                            />
                        </div>
                        <div className="w-7 flex justify-center items-center p-1 pr-0!">
                            <TrackAudioStatusIndicator />
                        </div>
                    </div>
                </>
            ) : radioConfig.integration === "AudioForVatsim" ? (
                <>
                    <RadioPttDescription callMicMode={transmitConfig.callMicMode} />
                    <p className="text-sm text-gray-800 leading-4.5">
                        Set the emit key as your PTT key in AFV. You do not press it yourself, vacs
                        will do so automatically for you. Choosing a rarely used key such as
                        ScrollLock helps avoid accidental triggers.
                    </p>
                    <div className="w-full flex flex-row gap-3 items-center justify-center">
                        <p className="text-sm w-[21ch]! shrink-0 text-right">Emit key:</p>
                        <KeyCapture
                            label={radioConfig.audioForVatsim?.emitLabel ?? null}
                            joystickEnabled={false}
                            onCapture={handleOnAfvEmitCapture}
                            onRemove={handleOnAfvEmitRemoveClick}
                        />
                    </div>
                </>
            ) : (
                <p className="w-full py-1 text-sm text-gray-800 leading-4.5">
                    <b>None: </b> No radio integration is configured. You can use vacs completely on
                    its own.
                    <br />
                    <b>TrackAudio: </b> vacs can connect to your TrackAudio client to trigger
                    transmissions, manage radio & frequency state and play back radio transmissions.
                    {capKeybindEmitter && (
                        <>
                            <br />
                            <b>Audio for Vatsim: </b> vacs simulates a key press for you to trigger
                            a radio transmission in AFV and records incoming radio calls. The radio
                            page is not available.
                        </>
                    )}
                </p>
            )}
        </div>
    );
}

function RadioPttDescription({callMicMode}: {callMicMode: CallMicMode}) {
    return (
        <p className="py-1 text-sm text-gray-800 leading-4.5 w-full min-h-14">
            The key configured above is your radio push to talk.{" "}
            <b>You must not bind it in your radio client.</b>{" "}
            {callMicMode === "VoiceActivation" && (
                <>
                    Pressing it will transmit your voice on frequency.{" "}
                    <span className="text-red-600 font-semibold">IMPORTANT:</span> Unless you
                    manually toggle <RadioPrioBadge />, your radio transmission will be heard during
                    a call.
                    <br />
                    <br />
                </>
            )}
            {callMicMode === "PushToTalk" && (
                <>
                    When using the same key as your call PTT, toggling <RadioPrioBadge /> allows you
                    to transmit on frequency during a call. Binding a different key lets you operate
                    the radio independently.
                </>
            )}
            {callMicMode === "PushToMute" && (
                <>
                    Pressing this key during a call will transmit on frequency without being audible
                    in the call. A different radio key is not supported in push-to-mute mode, hence
                    the key capture above is disabled.
                </>
            )}
        </p>
    );
}

const RadioStateAsIndicatorState: {[key in RadioState["state"]]: Status} = {
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
    const radioState = useRadioStore(state => state.radioState?.state ?? "NotConfigured");
    const canReconnect =
        radioState !== "NotConfigured" && (radioState === "Disconnected" || radioState === "Error");

    const handleButtonClick = useAsyncDebounce(async () => {
        if (canReconnect) {
            await invokeStrict("radio_reconnect");
        }
    });

    const title = canReconnect
        ? "Reconnect to TrackAudio"
        : radioState !== "NotConfigured"
          ? "Connected to TrackAudio"
          : "Deactivated";

    return (
        <StatusIndicator
            status={RadioStateAsIndicatorState[radioState]}
            className={canReconnect ? "cursor-pointer" : undefined}
            onClick={handleButtonClick}
            title={title}
        />
    );
}

export default RadioIntegrationSettings;
