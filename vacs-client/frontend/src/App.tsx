import Clock from "./components/Clock.tsx";
import InfoGrid from "./components/InfoGrid.tsx";
import FunctionKeys from "./components/FunctionKeys.tsx";
import CallQueue from "./components/CallQueue.tsx";
import {useEffect} from "preact/hooks";
import {invoke} from "./transport";
import {setupAuthListeners} from "./listeners/auth-listener.ts";
import telephone from "./assets/telephone.svg";
import ErrorOverlay from "./components/overlays/ErrorOverlay.tsx";
import {invokeSafe} from "./error.ts";
import {setupErrorListeners} from "./listeners/error-listener.ts";
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
import ProfileTabs from "./components/ProfileTabs.tsx";
import {useProfileStore, useProfileType} from "./stores/profile-store.ts";
import {fetchSettings} from "./stores/settings-store.ts";
import {usePageSync, useSplitView} from "./hooks/page-hook.ts";
import {useZoomHotkey} from "./hooks/zoom-hotkey-hook.ts";
import CplButton from "./components/ui/CplButton.tsx";
import {fetchRadioState, setupRadioListener} from "./listeners/radio-listener.ts";
import {setupPlaybackListener} from "./listeners/playback-listener.ts";
import Router from "./pages/Router.tsx";
import PageTabs from "./components/PageTabs.tsx";
import PageCycleButton from "./components/ui/PageCycleButton.tsx";

function App() {
    const connected = useConnectionStore(state => state.connectionState === "connected");
    const testing = useConnectionStore(state => state.connectionState === "test");
    const profileType = useProfileType();
    const profileView = useProfileStore(state => state.profile?.view);
    const splitView = useSplitView();

    useZoomHotkey();
    usePageSync();

    useEffect(() => {
        void invoke("app_frontend_ready");

        const cleanups: (() => void)[] = [];

        cleanups.push(setupErrorListeners());
        cleanups.push(setupAuthListeners());
        cleanups.push(setupSignalingListeners());
        cleanups.push(setupWebrtcListeners());
        cleanups.push(setupStoreSync());
        cleanups.push(setupRadioListener());
        cleanups.push(setupPlaybackListener());

        void invokeSafe("auth_check_session");

        void fetchCapabilities();
        void fetchSettings();
        void fetchRadioState();

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
                    <div className="relative h-full w-[calc(100%-6rem)] shrink-0 rounded-sm flex flex-row">
                        <Router />
                    </div>
                    {/* Right Button Row */}
                    <div className="w-24 h-full px-2 pb-6 flex flex-col justify-between">
                        <LinkButton menu="telephone" className="h-16 shrink-0">
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
                                {splitView && profileView === "split" ? (
                                    <PageTabs />
                                ) : splitView && profileView === "cycle" ? (
                                    <PageCycleButton />
                                ) : (
                                    <>
                                        <RadioButton />
                                        <PhoneButton />
                                    </>
                                )}
                                <CplButton />
                                <RadioPrioButton />
                            </>
                        ) : (
                            <>
                                <RadioButton />
                                <CplButton />
                                <RadioPrioButton />
                                <PhoneButton />
                            </>
                        )}
                    </div>
                    <div className="h-full flex flex-row gap-5">
                        {(connected || testing) && profileType === "tabbed" && <ProfileTabs />}
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
