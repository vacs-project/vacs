import {clsx} from "clsx";
import {useShallow} from "zustand/react/shallow";
import reload from "../assets/reload-cw.svg";
import Button from "../components/ui/Button.tsx";
import List from "../components/ui/List.tsx";
import {invokeSafe, invokeStrict} from "../error.ts";
import {useAsyncDebounce} from "../hooks/debounce-hook.ts";
import {useConnectionStore} from "../stores/connection-store.ts";
import {goToPage} from "../stores/navigation-store.ts";
import {useProfileStore} from "../stores/profile-store.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import {ClientGroupMode} from "../types/client.ts";

function MissionPage() {
    const isProfileSet = useProfileStore(state => state.profile !== undefined);

    return (
        <div className="z-10 absolute h-[calc(100%+5rem+5rem-0.5rem)] w-full translate-y-[calc(-4.75rem)] bg-blue-700 border-t-0 px-2 pb-2 flex flex-col overflow-auto rounded">
            <p className="w-full text-white bg-blue-700 font-semibold text-center">Mission</p>
            <div className="relative flex-1 min-h-0 flex flex-col rounded-b-sm bg-[#B5BBC6] ">
                <div className="relative w-full flex-1 min-h-0 flex justify-center py-3 px-2 items-center">
                    <ClientPageConfig />
                    {isProfileSet && (
                        <div className="absolute top-0 left-0 w-full h-full bg-[rgba(0,0,0,0.4)] z-10 flex justify-center items-center text-lg text-white">
                            Unavailable while using a profile
                        </div>
                    )}
                </div>
                <hr className="h-[2px] bg-white border-none" />
                <div className="w-full flex-1 min-h-0 flex justify-center py-3 px-2 items-center">
                    <p className="text-slate-600">Not implemented</p>
                </div>
                <TestProfile />
            </div>
        </div>
    );
}

function ClientPageConfig() {
    const configs = useSettingsStore(
        useShallow(state => Object.keys(state.clientPageConfigs).sort()),
    );
    const selectedConfig = useSettingsStore(state => state.selectedClientPageConfig);
    const setSelectedConfig = useSettingsStore(state => state.setClientPageConfig);

    const selectedConfigIndex = configs.indexOf(selectedConfig.name);

    const handleSelectStationsConfigClick = useAsyncDebounce(async () => {
        await invokeSafe("app_load_extra_client_page_config");
    });

    return (
        <>
            <div className="h-full flex flex-col gap-2">
                <List
                    className="w-full"
                    itemsCount={configs.length}
                    selectedItem={selectedConfigIndex}
                    setSelectedItem={async index => {
                        const configName = configs[index];
                        if (configName === undefined) return;
                        const config = useSettingsStore.getState().clientPageConfigs[configName];
                        if (config === undefined) return;
                        try {
                            await invokeStrict("app_set_selected_client_page_config", {
                                configName: configName !== "None" ? configName : undefined,
                            });
                            setSelectedConfig({...config, name: configName});
                        } catch {}
                    }}
                    defaultRows={6}
                    row={(index, isSelected, onClick) =>
                        ClientConfigRow(configs[index], isSelected, onClick)
                    }
                    header={[{title: "Configs"}]}
                    columnWidths={["1fr"]}
                />
                <Button
                    color="gray"
                    className="w-min whitespace-nowrap px-3 py-2 mr-16"
                    onClick={handleSelectStationsConfigClick}
                >
                    Select client page config
                </Button>
            </div>
            <div className="h-full ml-8 flex-1 flex flex-col">
                <p className="font-semibold truncate">Selected Config - {selectedConfig.name}</p>
                <div className="grid grid-cols-[auto_1fr] grid-rows-[auto_auto_auto_auto] gap-x-2 [&_p]:truncate">
                    <p>Include:</p>
                    <p>[{selectedConfig.include?.join(", ")}]</p>
                    <p>Exclude:</p>
                    <p>[{selectedConfig.exclude?.join(", ")}]</p>
                    <p>Priority:</p>
                    <p>[{selectedConfig.priority?.join(", ")}]</p>
                    <p>Frequencies:</p>
                    <p>{selectedConfig.frequencies === "HideAll" ? "Hide all" : "Show all"}</p>
                    <p>Grouping:</p>
                    <p>{GroupingLabels[selectedConfig.grouping]}</p>
                </div>
            </div>
        </>
    );
}

const GroupingLabels: {[key in ClientGroupMode]: string} = {
    None: "None",
    Fir: "FIR",
    Icao: "ICAO",
    FirAndIcao: "FIR and ICAO",
};

function ClientConfigRow(name: string | undefined, isSelected: boolean, onClick: () => void) {
    const color = isSelected ? "bg-blue-700 text-white" : "bg-yellow-50";

    return (
        <div className={clsx("px-0.5 flex items-center font-semibold", color)} onClick={onClick}>
            {name ?? ""}
        </div>
    );
}

function TestProfile() {
    const enableTestProfile = useConnectionStore(
        state => state.connectionState === "disconnected" || state.connectionState === "test",
    );
    const testProfilePath = useProfileStore(state => state.testProfilePath);
    const setTestProfilePath = useProfileStore(state => state.setTestProfilePath);
    const setConnectionState = useConnectionStore(state => state.setConnectionState);
    const resetProfileStore = useProfileStore(state => state.reset);

    const handleLoadTestProfileClick = useAsyncDebounce(async () => {
        try {
            const path = await invokeStrict<string>("app_load_test_profile");
            setTestProfilePath(path);
        } catch {}
    });

    const handleReloadClick = useAsyncDebounce(async () => {
        if (testProfilePath === undefined) return;
        await invokeStrict("app_load_test_profile", {path: testProfilePath});
    });

    const handleUnloadClick = useAsyncDebounce(async () => {
        void invokeSafe("app_unload_test_profile");
        setConnectionState("disconnected");
        resetProfileStore();
        setTestProfilePath(undefined);
        goToPage("phone");
    });

    return (
        <div className="absolute right-0 bottom-0 border-t-2 border-l-2 border-white p-3 flex gap-3">
            <Button
                color="gray"
                className="w-60 whitespace-nowrap px-3 py-2"
                disabled={!enableTestProfile}
                onClick={handleLoadTestProfileClick}
                title={
                    enableTestProfile
                        ? undefined
                        : "Cannot load test profile while being connected."
                }
            >
                Load test profile
            </Button>
            <Button
                color="gray"
                className="w-[2.625rem] whitespace-nowrap flex justify-center items-center"
                onClick={handleReloadClick}
                disabled={testProfilePath === undefined || !enableTestProfile}
            >
                <img src={reload} alt="Reload" />
            </Button>
            <Button
                color="gray"
                className="w-[2.625rem] whitespace-nowrap flex justify-center items-center"
                onClick={handleUnloadClick}
                disabled={testProfilePath === undefined || !enableTestProfile}
            >
                <svg
                    width="26"
                    height="26"
                    viewBox="0 0 128 128"
                    fill="none"
                    className="w-full"
                    xmlns="http://www.w3.org/2000/svg"
                >
                    <g clipPath="url(#clip0_0_1)">
                        <path d="M98 30L30 98" stroke="black" strokeWidth="12" />
                        <path d="M30 30L98 98" stroke="black" strokeWidth="12" />
                    </g>
                    <defs>
                        <clipPath id="clip0_0_1">
                            <rect
                                width="128"
                                height="128"
                                fill="white"
                                transform="matrix(-1 0 0 1 128 0)"
                            />
                        </clipPath>
                    </defs>
                </svg>
            </Button>
        </div>
    );
}

export default MissionPage;
