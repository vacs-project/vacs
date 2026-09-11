import {create} from "zustand/react";
import {invokeStrict} from "../error.ts";
import {isTauri} from "../transport";
import {ClientPageConfig, ClientPageSettings} from "../types/client.ts";
import {CallConfig, ClockMode, CplMode} from "../types/settings.ts";
import {
    RadioConfig,
    RadioConfigWithLabels,
    TransmitConfig,
    TransmitConfigWithLabels,
    withRadioLabels,
    withTransmitLabels,
} from "../types/transmit.ts";
import {useRadioStore} from "./radio-store.ts";
import {useStationsStore} from "./stations-store.ts";

type SettingsState = {
    callConfig: CallConfig;
    selectedClientPageConfig: ClientPageConfig & {name: string};
    clientPageConfigs: Record<string, ClientPageConfig>;
    transmitConfig: TransmitConfigWithLabels | undefined;
    radioConfig: RadioConfigWithLabels | undefined;
    clockMode: ClockMode;
    cplMode: CplMode;
    playbackEnabled: boolean;
    setCallConfig: (config: CallConfig) => void;
    setClientPageConfig: (config: ClientPageConfig & {name: string}) => void;
    setClientPageSettings: (settings: ClientPageSettings) => void;
    setTransmitConfig: (config: TransmitConfigWithLabels) => void;
    setRadioConfig: (config: RadioConfigWithLabels) => void;
    setClockMode: (mode: ClockMode) => void;
    setCplMode: (mode: CplMode) => void;
    setPlaybackEnabled: (enabled: boolean) => void;
};

const emptyClientPageConfig: ClientPageConfig = {
    include: [],
    exclude: [],
    priority: ["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND"],
    frequencies: "ShowAll",
    grouping: "FirAndIcao",
};

export const useSettingsStore = create<SettingsState>()(set => ({
    callConfig: {
        highlightIncomingCallTarget: true,
        enablePriorityCalls: true,
        enableCallStartSound: true,
        enableCallEndSound: true,
        useDefaultCallSources: true,
        forceRelay: false,
    },
    selectedClientPageConfig: {...emptyClientPageConfig, name: "None"},
    clientPageConfigs: {},
    transmitConfig: undefined,
    radioConfig: undefined,
    clockMode: "Realtime",
    cplMode: "Original",
    playbackEnabled: false,
    setCallConfig: config => set({callConfig: config}),
    setClientPageConfig: config => set({selectedClientPageConfig: config}),
    setClientPageSettings: ({selected, configs}) => {
        const resolvedConfig = isTauri && selected !== undefined ? configs[selected] : undefined;
        set({
            clientPageConfigs: {None: emptyClientPageConfig, ...configs},
            ...(resolvedConfig !== undefined && selected !== undefined
                ? {selectedClientPageConfig: {...resolvedConfig, name: selected}}
                : {}),
        });
    },
    setTransmitConfig: config => set({transmitConfig: config}),
    setRadioConfig: config => set({radioConfig: config}),
    setClockMode: mode => set({clockMode: mode}),
    setCplMode: mode => set({cplMode: mode}),
    setPlaybackEnabled: enabled => set({playbackEnabled: enabled}),
}));

useSettingsStore.subscribe((state, prev) => {
    if (state.callConfig.useDefaultCallSources !== prev.callConfig.useDefaultCallSources) {
        const {stations, positionDefaultSources, setDefaultSource, getPositionDefaultSource} =
            useStationsStore.getState();
        setDefaultSource(getPositionDefaultSource(positionDefaultSources, stations));
    }

    if (state.cplMode !== prev.cplMode && state.cplMode === "Fast") {
        const {cpl, setCpl} = useRadioStore.getState();
        if (cpl) setCpl(false);
    }
});

async function fetchClientPageConfigs() {
    try {
        const settings = await invokeStrict<ClientPageSettings>("app_get_client_page_settings");
        useSettingsStore.getState().setClientPageSettings(settings);
    } catch {}
}

export async function fetchSettings() {
    void fetchClientPageConfigs();

    if (!isTauri) return;

    try {
        const [callConfig, clockMode, cplMode, transmitConfig, radioConfig, playbackEnabled] =
            await Promise.all([
                invokeStrict<CallConfig>("app_get_call_config"),
                invokeStrict<ClockMode>("app_get_clock_mode"),
                invokeStrict<CplMode>("app_get_cpl_mode"),
                invokeStrict<TransmitConfig>("keybinds_get_transmit_config").then(
                    withTransmitLabels,
                ),
                invokeStrict<RadioConfig>("radio_get_config").then(withRadioLabels),
                invokeStrict<boolean>("playback_get_enabled"),
            ]);

        useSettingsStore.setState({
            callConfig,
            clockMode,
            cplMode,
            transmitConfig,
            radioConfig,
            playbackEnabled,
        });
    } catch {}
}
