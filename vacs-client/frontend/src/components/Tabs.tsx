import {clsx} from "clsx";
import Button from "./ui/Button.tsx";
import {useEffect, useState} from "preact/hooks";
import {invokeSafe} from "../error.ts";
import {useProfileStore} from "../stores/profile-store.ts";
import {useRoute} from "wouter";
import {navigate} from "wouter/use-browser-location";
import {Tab} from "../types/profile.ts";
import cycle from "../assets/cycle.svg";
import ButtonLabel from "./ui/ButtonLabel.tsx";

function Tabs() {
    const tabs = useProfileStore(state => state.profile?.tabbed);
    const setPage = useProfileStore(state => state.setPage);
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
                    }}
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

type TabButtonProps = {
    label: string[] | undefined;
    active?: boolean;
    onClick?: () => void;
};

function TabButton(props: TabButtonProps) {
    const [settingsOpen] = useRoute("/settings/*?");
    const disabled = props.label === undefined;

    return (
        <div className="w-20 relative">
            <button
                className={clsx(
                    "absolute -top-[calc(0.5rem+2px)] h-[calc(100%+0.5rem+2px)] w-20 rounded-b-lg border-t-0 font-semibold cursor-pointer leading-5",
                    "border-4 outline-2 outline-gray-700 -outline-offset-2 px-1.5 flex flex-col justify-center items-center c",
                    props.active &&
                        !settingsOpen &&
                        "active-tab border-b-gray-300 bg-linear-0/oklch from-gray-300 to-[#B5BBC6]",
                    disabled && "cursor-not-allowed! bg-gray-400",
                    (props.active && !settingsOpen) || disabled
                        ? "border-transparent"
                        : "bg-gray-300 border-l-gray-100 border-r-gray-700 border-b-gray-700 active:border-r-gray-100 active:border-b-gray-100 active:border-t-gray-700 active:border-l-gray-700 active:*:translate-y-px active:*:translate-x-px",
                )}
                disabled={(props.active && !settingsOpen) || disabled}
                onClick={() => {
                    void invokeSafe("audio_play_ui_click");
                    props.onClick?.();
                    if (settingsOpen) navigate("/");
                }}
            >
                {props.label && <ButtonLabel label={props.label} />}
            </button>
        </div>
    );
}

export default Tabs;
