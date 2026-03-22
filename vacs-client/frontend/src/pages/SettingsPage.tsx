import Button from "../components/ui/Button.tsx";
import {navigate} from "wouter/use-browser-location";
import {useAuthStore} from "../stores/auth-store.ts";
import {invokeSafe, invokeStrict} from "../error.ts";
import {useAsyncDebounce} from "../hooks/debounce-hook.ts";
import DeviceSelector from "../components/settings/DeviceSelector.tsx";
import VolumeSettings from "../components/settings/VolumeSettings.tsx";
import AudioHostSelector from "../components/settings/AudioHostSelector.tsx";
import {useEffect, useState} from "preact/hooks";
import {useUpdateStore} from "../stores/update-store.ts";
import {useCapabilitiesStore} from "../stores/capabilities-store.ts";
import {isTauri} from "../transport";
import {Route, Switch} from "wouter";
import TransmitModePage from "../components/settings/TransmitModePage.tsx";
import HotkeysConfigPage from "../components/settings/HotkeysConfigPage.tsx";
import {useConnectionStore} from "../stores/connection-store.ts";
import CallConfigPage from "../components/settings/CallConfigPage.tsx";
import AdvancedPage from "../components/settings/AdvancedPage.tsx";

function SettingsPage() {
    return (
        <>
            <div className="h-full w-full bg-blue-700 border-t-0 px-2 pb-2 flex flex-col">
                <p className="w-full text-white bg-blue-700 font-semibold text-center">Settings</p>
                <div className="w-full grow rounded-b-sm bg-[#B5BBC6] flex flex-col overflow-auto">
                    <div className="w-full grow border-b-2 border-zinc-200 flex flex-row">
                        <VolumeSettings />
                        <div className="h-full grow flex flex-col">
                            <p className="w-full text-center border-b-2 border-zinc-200 uppercase font-semibold">
                                Devices
                            </p>
                            <div className="w-full px-3 py-1.5 flex flex-col">
                                <AudioHostSelector />
                                <DeviceSelector deviceType="Output" />
                                <DeviceSelector deviceType="Input" />
                            </div>
                            <div className="py-0.5 flex flex-col gap-2">
                                <p className="pt-1 text-center font-semibold uppercase border-t-2 border-zinc-200">
                                    Miscellaneous
                                </p>
                                <div className="px-3 pb-2 grid grid-cols-[repeat(3,auto)] justify-center grid-rows-2 gap-4 [&>button]:h-16">
                                    <UpdateButton />
                                    <Button
                                        color="gray"
                                        className="h-full text-sm"
                                        onClick={() =>
                                            invokeSafe("app_open_folder", {folder: "Config"})
                                        }
                                    >
                                        Open
                                        <br />
                                        Config
                                    </Button>
                                    <Button
                                        color="gray"
                                        className="h-full text-sm"
                                        onClick={() =>
                                            invokeSafe("app_open_folder", {folder: "Logs"})
                                        }
                                    >
                                        Open
                                        <br />
                                        Logs
                                    </Button>

                                    <WindowStateButtons />
                                </div>
                            </div>
                        </div>
                    </div>
                    <div className="h-20 w-full flex flex-row gap-2 justify-between p-2 [&>button]:px-1 [&>button]:shrink-0">
                        <div className="h-full flex flex-row gap-2 items-center">
                            <Button
                                color="gray"
                                className="w-22 h-full text-sm"
                                onClick={() => navigate("/settings/transmit")}
                            >
                                Transmit
                            </Button>
                            <Button
                                color="gray"
                                className="w-20 h-full text-sm"
                                onClick={() => navigate("/settings/hotkeys")}
                            >
                                Hotkeys
                            </Button>
                            <Button
                                color="gray"
                                className="w-20 h-full text-sm"
                                onClick={() => navigate("/settings/call")}
                            >
                                Call
                            </Button>
                            <Button
                                color="gray"
                                className="w-22 h-full text-sm"
                                onClick={() => navigate("/settings/advanced")}
                            >
                                Advanced
                            </Button>
                        </div>
                        <AppControlButtons />
                    </div>
                </div>
            </div>
            <Switch>
                <Route path="/transmit" component={TransmitModePage} />
                <Route path="/hotkeys" component={HotkeysConfigPage} />
                <Route path="/call" component={CallConfigPage} />
                <Route path="/advanced" component={AdvancedPage} />
            </Switch>
        </>
    );
}

function AppControlButtons() {
    const connected = useConnectionStore(state => state.connectionState === "connected");
    const isAuthenticated = useAuthStore(state => state.status === "authenticated");

    const handleLogoutClick = useAsyncDebounce(async () => {
        try {
            await invokeStrict("auth_logout");
            navigate("/");
        } catch {}
    });

    const handleDisconnectClick = useAsyncDebounce(async () => {
        try {
            await invokeStrict("signaling_disconnect");
            navigate("/");
        } catch {}
    });

    const handleQuitClick = useAsyncDebounce(async () => {
        await invokeSafe("app_quit");
    });

    return (
        <div className="h-full flex flex-row gap-2">
            <Button
                color="salmon"
                className="w-auto px-3 text-sm"
                disabled={!connected}
                onClick={handleDisconnectClick}
            >
                Disconnect
            </Button>
            <Button
                color="salmon"
                className="text-sm"
                disabled={!isAuthenticated}
                onClick={handleLogoutClick}
            >
                Logout
            </Button>
            <Button color="salmon" muted={true} className="text-sm ml-3" onClick={handleQuitClick}>
                Quit
            </Button>
        </div>
    );
}

function UpdateButton() {
    const [noNewVersion, setNoNewVersion] = useState<boolean>(false);
    const newVersion = useUpdateStore(state => state.newVersion);
    const {
        setVersions: setUpdateVersions,
        openMandatoryDialog,
        openDownloadDialog,
        closeOverlay,
    } = useUpdateStore(state => state.actions);

    const handleOnClick = useAsyncDebounce(async () => {
        if (newVersion !== undefined) {
            try {
                openDownloadDialog();
                await invokeStrict("app_update");
            } catch {
                closeOverlay();
            }
        } else {
            const checkUpdateResult = await invokeSafe<{
                currentVersion: string;
                newVersion?: string;
                required: boolean;
            }>("app_check_for_update");
            if (checkUpdateResult === undefined) return;

            setUpdateVersions(checkUpdateResult.currentVersion, checkUpdateResult.newVersion);

            if (checkUpdateResult.required) {
                openMandatoryDialog();
            } else {
                if (checkUpdateResult.newVersion === undefined) {
                    setNoNewVersion(true);
                }
                closeOverlay();
            }
        }
    });

    return (
        <Button
            color={newVersion === undefined ? "gray" : "green"}
            className="w-24 h-full rounded text-sm"
            onClick={handleOnClick}
            disabled={noNewVersion}
        >
            {newVersion === undefined ? (
                noNewVersion ? (
                    <p>
                        No Update
                        <br />
                        available
                    </p>
                ) : (
                    <p>
                        Check for
                        <br />
                        Updates
                    </p>
                )
            ) : (
                <p>
                    Update &<br />
                    Restart
                </p>
            )}
        </Button>
    );
}

function WindowStateButtons() {
    const [alwaysOnTop, setAlwaysOnTop] = useState<boolean>(false);
    const [fullscreen, setFullscreen] = useState<boolean>(false);

    const capAlwaysOnTop = useCapabilitiesStore(state => state.alwaysOnTop);
    const capPlatform = useCapabilitiesStore(state => state.platform);

    const toggleAlwaysOnTop = useAsyncDebounce(async () => {
        const isAlwaysOnTop = await invokeSafe<boolean>("app_set_always_on_top", {
            alwaysOnTop: !alwaysOnTop,
        });
        setAlwaysOnTop(alwaysOnTop => isAlwaysOnTop ?? alwaysOnTop);
    });

    const toggleFullscreen = useAsyncDebounce(async () => {
        const isFullscreen = await invokeSafe<boolean>("app_set_fullscreen", {
            fullscreen: !fullscreen,
        });
        setFullscreen(isFullscreen ?? false);
    });

    const handleResetWindowSizeClick = useAsyncDebounce(async () => {
        try {
            await invokeStrict("app_reset_window_size");
            setFullscreen(false);
        } catch {}
    });

    useEffect(() => {
        if (!isTauri) return;
        const fetchWindowState = async () => {
            const {getCurrentWindow} = await import("@tauri-apps/api/window");
            const isAlwaysOnTop = await getCurrentWindow().isAlwaysOnTop();
            setAlwaysOnTop(prev => isAlwaysOnTop ?? prev);
            const isFs = await getCurrentWindow().isFullscreen();
            setFullscreen(isFs);
        };

        void fetchWindowState();
    }, []);

    return (
        <>
            <Button
                color={alwaysOnTop ? "blue" : "cyan"}
                className="h-full w-24 rounded text-sm"
                onClick={toggleAlwaysOnTop}
                disabled={!capAlwaysOnTop}
                title={
                    !capAlwaysOnTop
                        ? `Unfortunately, always-on-top is not yet supported on ${capPlatform}`
                        : undefined
                }
            >
                <p>
                    Always
                    <br />
                    on Top
                </p>
            </Button>
            <Button
                color={fullscreen ? "blue" : "cyan"}
                className="h-full rounded text-sm"
                onClick={toggleFullscreen}
            >
                <p>
                    Full
                    <br />
                    Screen
                </p>
            </Button>
            <Button
                color="gray"
                className="h-full rounded text-sm"
                onClick={handleResetWindowSizeClick}
            >
                <p>Reset Size</p>
            </Button>
        </>
    );
}

export function CloseButton() {
    return (
        <Button color="gray" className="w-18!" onClick={() => navigate("/settings")}>
            <svg
                width="26"
                height="26"
                viewBox="0 0 128 128"
                fill="none"
                className="w-full"
                xmlns="http://www.w3.org/2000/svg"
            >
                <g clipPath="url(#clip0_0_1)">
                    <rect x="4" y="4" width="120" height="120" stroke="black" strokeWidth="14" />
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
    );
}

export default SettingsPage;
