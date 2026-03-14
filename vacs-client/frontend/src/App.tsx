import Clock from "./components/Clock.tsx";
import InfoGrid from "./components/InfoGrid.tsx";
import FunctionKeys from "./components/FunctionKeys.tsx";
import CallQueue from "./components/CallQueue.tsx";
import {useEffect} from "preact/hooks";
import {invoke} from "./transport";
import {Route, Switch} from "wouter";
import LoginPage from "./pages/LoginPage.tsx";
import {useAuthStore} from "./stores/auth-store.ts";
import {setupAuthListeners} from "./listeners/auth-listener.ts";
import ConnectPage from "./pages/ConnectPage.tsx";
import SettingsPage from "./pages/SettingsPage.tsx";
import telephone from "./assets/telephone.svg";
import ErrorOverlay from "./components/overlays/ErrorOverlay.tsx";
import {invokeSafe} from "./error.ts";
import {setupErrorListeners} from "./listeners/error-listener.ts";
import MissionPage from "./pages/MissionPage.tsx";
import TelephonePage from "./pages/TelephonePage.tsx";
import LinkButton from "./components/ui/LinkButton.tsx";
import {setupSignalingListeners} from "./listeners/signaling-listener.ts";
import PhoneButton from "./components/ui/PhoneButton.tsx";
import RadioPrioButton from "./components/ui/RadioPrioButton.tsx";
import EndButton from "./components/ui/EndButton.tsx";
import {setupWebrtcListeners} from "./listeners/webrtc-listener.ts";
import {setupStoreSync} from "./transport/store-sync.ts";
import UpdateOverlay from "./components/overlays/UpdateOverlay.tsx";
import {fetchCapabilities} from "./stores/capabilities-store.ts";
import RadioButton from "./components/ui/RadioButton.tsx";
import ConnectionTerminateOverlay from "./components/overlays/ConnectionTerminateOverlay.tsx";
import {useConnectionStore} from "./stores/connection-store.ts";
import PositionSelectOverlay from "./components/overlays/PositionSelectOverlay.tsx";
import MainPage from "./pages/MainPage.tsx";
import Tabs from "./components/Tabs.tsx";
import {useProfileType} from "./stores/profile-store.ts";
import Button from "./components/ui/Button.tsx";
import {fetchCallConfig, fetchClientPageSettings} from "./stores/settings-store.ts";
import {useZoomHotkey} from "./hooks/zoom-hotkey-hook.ts";

function App() {
    const connected = useConnectionStore(state => state.connectionState === "connected");
    const testing = useConnectionStore(state => state.connectionState === "test");
    const authStatus = useAuthStore(state => state.status);
    const profileType = useProfileType();
    useZoomHotkey();

    useEffect(() => {
        void invoke("app_frontend_ready");

        const cleanups: (() => void)[] = [];

        cleanups.push(setupErrorListeners());
        cleanups.push(setupAuthListeners());
        cleanups.push(setupSignalingListeners());
        cleanups.push(setupWebrtcListeners());
        cleanups.push(setupStoreSync());

        void invokeSafe("auth_check_session");

        void fetchCapabilities();
        void fetchCallConfig();
        void fetchClientPageSettings();

        return () => {
            cleanups.forEach(cleanup => cleanup());
        };
    }, []);

    return (
        <div className="h-full flex flex-col">
            <div className="w-full h-12 bg-gray-300 flex flex-row border-gray-700 border-b">
                <Clock />
                <InfoGrid />
            </div>
            <div className="w-full h-[calc(100%-3rem)] flex flex-col">
                {/* Top Button Row */}
                <FunctionKeys />
                <div className="flex flex-row w-full h-[calc(100%-10rem)] pl-1">
                    {/* Main Area */}
                    <div className="relative h-full w-[calc(100%-6rem)] bg-[#B5BBC6] border-l border-t border-r-2 border-b-2 border-gray-700 rounded-sm flex flex-row">
                        <Switch>
                            <Route path="/settings" component={SettingsPage} nest />
                            <Route path="/mission" component={MissionPage} />
                            <Route path="/" nest>
                                {authStatus === "loading" ? (
                                    <></>
                                ) : authStatus === "unauthenticated" && !testing ? (
                                    <LoginPage />
                                ) : connected || testing ? (
                                    <MainPage />
                                ) : (
                                    <ConnectPage />
                                )}
                                <Route path="/telephone" component={TelephonePage} />
                            </Route>
                        </Switch>
                    </div>
                    {/* Right Button Row */}
                    <div className="w-24 h-full px-2 pb-6 flex flex-col justify-between">
                        <LinkButton path="/telephone" className="h-16 shrink-0">
                            <img
                                src={telephone}
                                alt="Telephone"
                                className="h-18 w-18"
                                draggable={false}
                            />
                        </LinkButton>
                        <CallQueue />
                    </div>
                </div>
                {/* Bottom Button Row */}
                <div className="h-20 w-full p-2 pl-4 flex flex-row justify-between">
                    <div className="h-full flex flex-row gap-3">
                        {profileType === "tabbed" ? (
                            <>
                                <RadioButton />
                                <PhoneButton />
                                <RadioPrioButton />
                            </>
                        ) : (
                            <>
                                <RadioButton />
                                <Button
                                    color="cyan"
                                    className="text-xl text-slate-400"
                                    disabled={true}
                                >
                                    CPL
                                </Button>
                                <RadioPrioButton />
                                <PhoneButton />
                            </>
                        )}
                    </div>
                    <div className="h-full flex flex-row gap-5">
                        {(connected || testing) && profileType === "tabbed" && <Tabs />}
                        <EndButton />
                    </div>
                </div>
            </div>
            <ErrorOverlay />
            <UpdateOverlay />
            <ConnectionTerminateOverlay />
            <PositionSelectOverlay />
        </div>
    );
}

export default App;
