import {useNavigationStore} from "../stores/navigation-store.ts";
import SettingsPage from "./SettingsPage.tsx";
import MissionPage from "./MissionPage.tsx";
import TelephonePage from "./TelephonePage.tsx";
import PhonePage from "./PhonePage.tsx";
import RadioPage from "./RadioPage.tsx";
import PlaybackPage from "./PlaybackPage.tsx";
import {ComponentChildren} from "preact";

function Router() {
    const page = useNavigationStore(state => state.page);
    const menu = useNavigationStore(state => state.menu);

    const hidePage = menu === "settings" || menu === "mission";

    return (
        <>
            {menu === "settings" ? (
                <SettingsPage />
            ) : menu === "mission" ? (
                <MissionPage />
            ) : menu === "telephone" ? (
                <TelephonePage />
            ) : menu === "playback" ? (
                <PlaybackPage />
            ) : (
                <></>
            )}
            {!hidePage &&
                (page === "phone" ? (
                    <MainWrap>
                        <PhonePage />
                    </MainWrap>
                ) : (
                    <MainWrap>
                        <RadioPage />
                    </MainWrap>
                ))}
        </>
    );
}

function MainWrap({children}: {children: ComponentChildren}) {
    return (
        <div className="relative h-full flex-1 min-w-0 bg-[#B5BBC6] border-l border-t border-r-2 border-b-2 border-gray-700 rounded-sm flex flex-row">
            {children}
        </div>
    );
}

export default Router;
