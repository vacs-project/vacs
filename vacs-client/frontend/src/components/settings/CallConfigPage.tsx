import {invokeStrict} from "../../error.ts";
import {useSettingsStore} from "../../stores/settings-store.ts";
import {CallConfig} from "../../types/settings.ts";
import Checkbox from "../ui/Checkbox.tsx";
import RingSoundSettings from "./RingSoundSettings.tsx";
import SettingsSubPage from "./SettingsSubPage.tsx";

function CallConfigPage() {
    const callConfig = useSettingsStore(state => state.callConfig);
    const setCallConfig = useSettingsStore(state => state.setCallConfig);

    return (
        <SettingsSubPage
            title="Call Config"
            width="w-[69%]"
            className="grid grid-cols-2 content-start"
        >
            <div className="flex flex-col">
                <p className="w-full text-center border-b-2 border-zinc-200 uppercase font-semibold">
                    General
                </p>
                <div className="py-3 px-4 flex flex-col gap-3">
                    <CallConfigEntry
                        label="Highlight incoming target"
                        name="display-call-target"
                        property="highlightIncomingCallTarget"
                        callConfig={callConfig}
                        setCallConfig={setCallConfig}
                    />
                    <CallConfigEntry
                        label="Enable priority calls"
                        name="enable-priority-calls"
                        property="enablePriorityCalls"
                        callConfig={callConfig}
                        setCallConfig={setCallConfig}
                    />
                    <CallConfigEntry
                        label="Use default call sources"
                        name="use-default-call-sources"
                        property="useDefaultCallSources"
                        callConfig={callConfig}
                        setCallConfig={setCallConfig}
                    />
                    <CallConfigEntry
                        label="Always relay calls"
                        name="force-relay"
                        property="forceRelay"
                        callConfig={callConfig}
                        setCallConfig={setCallConfig}
                    />
                    <CallConfigEntry
                        label="End active call on accept"
                        name="end-active-call-on-accept"
                        property="endActiveCallOnAccept"
                        callConfig={callConfig}
                        setCallConfig={setCallConfig}
                    />
                </div>
            </div>
            <div className="flex flex-col border-l-2 border-zinc-200">
                <p className="w-full text-center border-b-2 border-zinc-200 uppercase font-semibold">
                    Sounds
                </p>
                <div className="py-3 px-4 flex flex-col gap-3">
                    <CallConfigEntry
                        label="Call start"
                        name="enable-call-start-sound"
                        property="enableCallStartSound"
                        callConfig={callConfig}
                        setCallConfig={setCallConfig}
                    />
                    <CallConfigEntry
                        label="Call end"
                        name="enable-call-end-sound"
                        property="enableCallEndSound"
                        callConfig={callConfig}
                        setCallConfig={setCallConfig}
                    />
                    <CallConfigEntry
                        label="Participant joined"
                        name="enable-participant-joined-sound"
                        property="enableParticipantJoinedSound"
                        callConfig={callConfig}
                        setCallConfig={setCallConfig}
                    />
                    <CallConfigEntry
                        label="Participant left"
                        name="enable-participant-left-sound"
                        property="enableParticipantLeftSound"
                        callConfig={callConfig}
                        setCallConfig={setCallConfig}
                    />
                </div>
            </div>
            <div className="col-span-2">
                <RingSoundSettings />
            </div>
        </SettingsSubPage>
    );
}

type CallConfigEntryProps = {
    label: string;
    name: string;
    callConfig: CallConfig;
    setCallConfig: (config: CallConfig) => void;
    property: keyof CallConfig;
};

function CallConfigEntry(props: CallConfigEntryProps) {
    return (
        <div className="w-full flex justify-between items-center">
            <label htmlFor={props.name}>{props.label}</label>
            <Checkbox
                name={props.name}
                checked={props.callConfig[props.property]}
                muted={
                    (props.property === "enableCallEndSound" ||
                        props.property === "enableCallStartSound") &&
                    !props.callConfig[props.property]
                }
                onChange={async event => {
                    const next = event.currentTarget.checked;
                    const config = {
                        ...props.callConfig,
                        [props.property]: next,
                    };

                    try {
                        await invokeStrict("app_set_call_config", {callConfig: config});
                        props.setCallConfig(config);
                    } catch {
                        props.setCallConfig({
                            ...props.callConfig,
                            [props.property]: !next,
                        });
                    }
                }}
            />
        </div>
    );
}

export default CallConfigPage;
