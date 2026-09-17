import clsx from "clsx";
import {invokeSafe} from "../../error";
import {closeMenu} from "../../stores/navigation-store";
import ButtonLabel from "./ButtonLabel";

type TabButtonProps = {
    label: string[] | undefined;
    active?: boolean;
    onClick?: () => void;
    tabViewHidden?: boolean;
};

function TabButton(props: TabButtonProps) {
    const disabled = props.label === undefined;

    return (
        <div className="w-20 relative">
            <button
                className={clsx(
                    "absolute -top-[calc(0.5rem+2px)] h-[calc(100%+0.5rem+2px)] w-20 rounded-b-lg border-t-0 font-semibold cursor-pointer leading-5",
                    "border-4 outline-2 outline-gray-700 -outline-offset-2 px-1.5 flex flex-col justify-center items-center c",
                    props.active &&
                        !props.tabViewHidden &&
                        "active-tab border-b-gray-300 bg-linear-0/oklch from-gray-300 to-[#B5BBC6]",
                    disabled && "cursor-not-allowed! bg-gray-400",
                    (props.active && !props.tabViewHidden) || disabled
                        ? "border-transparent"
                        : "bg-gray-300 border-l-gray-100 border-r-gray-700 border-b-gray-700 active:border-r-gray-100 active:border-b-gray-100 active:border-t-gray-700 active:border-l-gray-700 active:*:translate-y-px active:*:translate-x-px",
                )}
                disabled={(props.active && !props.tabViewHidden) || disabled}
                onClick={() => {
                    void invokeSafe("audio_play_ui_click");
                    props.onClick?.();
                    if (props.tabViewHidden) closeMenu();
                }}
            >
                {props.label && <ButtonLabel label={props.label} />}
            </button>
        </div>
    );
}

export default TabButton;
