import {useNavigationStore} from "../stores/navigation-store.ts";
import SettingsPage from "./SettingsPage.tsx";
import MissionPage from "./MissionPage.tsx";
import TelephonePage from "./TelephonePage.tsx";
import PhonePage from "./PhonePage.tsx";
import RadioPage from "./RadioPage.tsx";
import PlaybackPage from "./PlaybackPage.tsx";
import {ComponentChildren} from "preact";
import clsx from "clsx";

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
                (page === "split" ? (
                    <>
                        <MainWrap>
                            <RadioPage />
                        </MainWrap>
                        <MainWrap width="calc(5.5rem * 4 + 2rem + 3px)">
                            <PhonePage />
                        </MainWrap>
                    </>
                ) : page === "phone" ? (
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

function MainWrap({children, width}: {children: ComponentChildren; width?: string}) {
    return (
        <div
            className={clsx(
                "relative h-full shrink-0 bg-[#B5BBC6] border-l border-t border-r-2 border-b-2 border-gray-700 rounded-sm flex flex-row",
                width === undefined && "flex-1 min-w-0",
            )}
            style={{width: width}}
        >
            {children}
        </div>
    );
}

export default Router;
