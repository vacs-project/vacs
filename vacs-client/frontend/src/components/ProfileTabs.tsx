import {clsx} from "clsx";
import {useEffect, useState} from "preact/hooks";
import cycle from "../assets/cycle.svg";
import {useSplitView} from "../hooks/page-hook.ts";
import {setPage as setNavigationPage, useNavigationStore} from "../stores/navigation-store.ts";
import {useProfileStore} from "../stores/profile-store.ts";
import {Tab} from "../types/profile.ts";
import Button from "./ui/Button.tsx";
import TabButton from "./ui/TabButton.tsx";

function ProfileTabs() {
    const tabs = useProfileStore(state => state.profile?.tabbed);
    const setPage = useProfileStore(state => state.setPage);
    const navigationPage = useNavigationStore(state => state.page);
    const settingsOpen = useNavigationStore(state => state.menu === "settings");
    const splitView = useSplitView();
    const [active, setActive] = useState<number>(0);
    const [offset, setOffset] = useState<number>(0);

    const [visible, setVisible] = useState<boolean>(false);

    useEffect(() => {
        setTimeout(() => {
            setVisible(true);
        }, 150);
    }, [setVisible]);

    useEffect(() => {
        if (tabs === undefined) return;
        const tab = tabs[active + offset];
        if (tab !== undefined) {
            setPage(tab.page);
        } else {
            setActive(0);
            setOffset(0);
            setPage(tabs[0].page);
        }
    }, [active, offset, tabs, setPage]);

    if (tabs === undefined) return <></>;

    const visibleTabs = (() => {
        const visibleTabs: (Tab | undefined)[] = tabs.slice(offset, offset + 4);
        const toFill = Math.min(tabs.length, 4) - visibleTabs.length;
        for (let i = 0; i < toFill; i++) visibleTabs.push(undefined);
        return visibleTabs;
    })();

    return (
        <div className={clsx("h-full flex flex-row", !visible && "hidden")}>
            {tabs.length > 4 && (
                <Button
                    color="gray"
                    className={clsx("w-20 h-full mr-1")}
                    onClick={() => {
                        setOffset(o => {
                            const next = getNextOffset(o, tabs.length);

                            if (tabs[active + next] === undefined) {
                                setActive((tabs.length - 1) % 4);
                            }

                            return next;
                        });
                    }}
                >
                    <div className="w-full h-full flex flex-col items-center">
                        <img src={cycle} alt="<->" className="w-9 h-9" />
                        <p className="">DA {daSwitchLabel(offset, tabs.length)}</p>
                    </div>
                </Button>
            )}
            {visibleTabs.map((tab, index) => (
                <TabButton
                    key={index}
                    label={tab?.label}
                    active={active === index}
                    onClick={() => {
                        setActive(index);
                        if (navigationPage === "radio") {
                            setNavigationPage(splitView ? "split" : "phone");
                        }
                    }}
                    tabViewHidden={settingsOpen || navigationPage === "radio"}
                />
            ))}
        </div>
    );
}

function getNextOffset(offset: number, tabsLength: number) {
    return offset >= tabsLength - 4 ? 0 : offset + 4;
}

function daSwitchLabel(offset: number, tabsLength: number) {
    const from = getNextOffset(offset, tabsLength) + 1;
    const to = Math.min(tabsLength, from + 3);
    return from !== to ? `${from}-${to}` : from;
}

export default ProfileTabs;
