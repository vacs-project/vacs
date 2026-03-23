import {create} from "zustand/react";
import {invokeStrict} from "../error.ts";
import {CallConfig} from "../types/settings.ts";
import {ClientPageConfig, ClientPageSettings} from "../types/client.ts";
import {useStationsStore} from "./stations-store.ts";

type SettingsState = {
    callConfig: CallConfig;
    selectedClientPageConfig: ClientPageConfig & {name: string};
    clientPageConfigs: Record<string, ClientPageConfig>;
    setCallConfig: (config: CallConfig) => void;
    setClientPageConfig: (config: ClientPageConfig & {name: string}) => void;
    setClientPageSettings: (settings: ClientPageSettings) => void;
};

const emptyClientPageConfig: ClientPageConfig = {
    include: [],
    exclude: [],
    priority: ["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND"],
    frequencies: "ShowAll",
    grouping: "FirAndIcao",
};

export const useSettingsStore = create<SettingsState>()((set, get) => ({
    callConfig: {
        highlightIncomingCallTarget: true,
        enablePriorityCalls: true,
        enableCallStartSound: true,
        enableCallEndSound: true,
        useDefaultCallSources: true,
    },
    selectedClientPageConfig: {...emptyClientPageConfig, name: "None"},
    clientPageConfigs: {},
    setCallConfig: config => {
        const defaultCallSourcesChanged =
            config.useDefaultCallSources !== get().callConfig.useDefaultCallSources;

        set({callConfig: config});

        if (defaultCallSourcesChanged) {
            const {stations, positionDefaultSources, setDefaultSource, getPositionDefaultSource} =
                useStationsStore.getState();

            setDefaultSource(getPositionDefaultSource(positionDefaultSources, stations));
        }
    },
    setClientPageConfig: config => {
        set({selectedClientPageConfig: config});
    },
    setClientPageSettings: settings => {
        set({clientPageConfigs: {None: emptyClientPageConfig, ...settings.configs}});

        if (settings.selected !== undefined) {
            const config = settings.configs[settings.selected];
            if (config !== undefined) {
                useSettingsStore
                    .getState()
                    .setClientPageConfig({...config, name: settings.selected});
            }
        }
    },
}));

export async function fetchCallConfig() {
    try {
        const callConfig = await invokeStrict<CallConfig>("app_get_call_config");

        useSettingsStore.getState().setCallConfig(callConfig);
    } catch {}
}

export async function fetchClientPageSettings() {
    try {
        const clientPageSettings = await invokeStrict<ClientPageSettings>(
            "app_get_client_page_settings",
        );
        useSettingsStore.getState().setClientPageSettings(clientPageSettings);
    } catch {}
}
