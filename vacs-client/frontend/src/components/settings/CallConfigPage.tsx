import Checkbox from "../ui/Checkbox.tsx";
import {useSettingsStore} from "../../stores/settings-store.ts";
import {invokeStrict} from "../../error.ts";
import {CallConfig} from "../../types/settings.ts";
import SettingsSubPage from "./SettingsSubPage.tsx";

function CallConfigPage() {
    const callConfig = useSettingsStore(state => state.callConfig);
    const setCallConfig = useSettingsStore(state => state.setCallConfig);

    return (
        <SettingsSubPage
            title="Call Config"
            width="w-2/5"
            className="py-3 px-4 flex flex-col gap-3"
        >
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
                label="Play call start sound"
                name="enable-call-start-sound"
                property="enableCallStartSound"
                callConfig={callConfig}
                setCallConfig={setCallConfig}
            />
            <CallConfigEntry
                label="Play call end sound"
                name="enable-call-end-sound"
                property="enableCallEndSound"
                callConfig={callConfig}
                setCallConfig={setCallConfig}
            />
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
