import {clsx} from "clsx";
import {ComponentChildren, CSSProperties} from "preact";
import {invokeSafe} from "../../error.ts";

export type ButtonColor =
    | "gray"
    | "cyan"
    | "green"
    | "blue"
    | "cornflower"
    | "emerald"
    | "red"
    | "salmon"
    | "peach"
    | "honey"
    | "sage"
    | "yellow"
    | DirectAccessKeyColors;
export type DirectAccessKeyColors =
    | "clay"
    | "blush"
    | "lilac"
    | "mint"
    | "periwinkle"
    | "taupe"
    | "orchid"
    | "steel"
    | "umber"
    | "lagoon";
export type ButtonHighlightColor = "green" | "gray";

export type ButtonProps = {
    color: ButtonColor;
    className?: string;
    children?: ComponentChildren;
    onClick?: (event: MouseEvent) => void;
    disabled?: boolean;
    softDisabled?: boolean;
    muted?: boolean;
    highlight?: ButtonHighlightColor;
    title?: string;
    style?: CSSProperties;
};

const DirectAccessKeyButtonColors: Record<DirectAccessKeyColors, string> = {
    // : "bg-[] border-t-[TL] border-l-[TL] border-r-[BR] border-b-[BR]",
    // : "active:border-r-[TL] active:border-b-[TL] active:border-t-[BR] active:border-l-[BR]",
    // : "border-[BR]! border!",
    clay: "bg-[#e68765] border-t-[#eb9f84] border-l-[#eb9f84] border-r-[#b86c51] border-b-[#b86c51]",
    blush: "bg-[#ebc3bc] border-t-[#f0d2cd] border-l-[#f0d2cd] border-r-[#b0928d] border-b-[#b0928d]",
    lilac: "bg-[#db9acc] border-t-[#e4b3d9] border-l-[#e4b3d9] border-r-[#a47499] border-b-[#a47499]",
    mint: "bg-[#abdecc] border-t-[#c0e6d9] border-l-[#c0e6d9] border-r-[#80a799] border-b-[#80a799]",
    // moss: "bg-[#c4deab] border-t-[#d3e6c0] border-l-[#d3e6c0] border-r-[#93a780] border-b-[#93a780]",
    periwinkle:
        "bg-[#b9abde] border-t-[#cbc0e6] border-l-[#cbc0e6] border-r-[#8b80a7] border-b-[#8b80a7]",
    taupe: "bg-[#bba58f] border-t-[#ccbcab] border-l-[#ccbcab] border-r-[#8c7c6b] border-b-[#8c7c6b]",
    orchid: "bg-[#cf78a9] border-t-[#db9abf] border-l-[#db9abf] border-r-[#9b5a7f] border-b-[#9b5a7f]",
    steel: "bg-[#8fa6b4] border-t-[#abbcc7] border-l-[#abbcc7] border-r-[#6b7d87] border-b-[#6b7d87]",
    umber: "bg-[#a98874] border-t-[#bca391] border-l-[#bca391] border-r-[#7e6655] border-b-[#7e6655]",
    lagoon: "bg-[#73b7c2] border-t-[#95cad1] border-l-[#95cad1] border-r-[#598b92] border-b-[#598b92]",
};

const DirectAccessKeyActiveButtonColors: Record<DirectAccessKeyColors, string> = {
    clay: "active:border-r-[#eb9f84] active:border-b-[#eb9f84] active:border-t-[#b86c51] active:border-l-[#b86c51]",
    blush: "active:border-r-[#f0d2cd] active:border-b-[#f0d2cd] active:border-t-[#b0928d] active:border-l-[#b0928d]",
    lilac: "active:border-r-[#e4b3d9] active:border-b-[#e4b3d9] active:border-t-[#a47499] active:border-l-[#a47499]",
    mint: "active:border-r-[#c0e6d9] active:border-b-[#c0e6d9] active:border-t-[#80a799] active:border-l-[#80a799]",
    // moss: "active:border-r-[#d3e6c0] active:border-b-[#d3e6c0] active:border-t-[#93a780] active:border-l-[#93a780]",
    periwinkle:
        "active:border-r-[#cbc0e6] active:border-b-[#cbc0e6] active:border-t-[#8b80a7] active:border-l-[#8b80a7]",
    taupe: "active:border-r-[#ccbcab] active:border-b-[#ccbcab] active:border-t-[#8c7c6b] active:border-l-[#8c7c6b]",
    orchid: "active:border-r-[#db9abf] active:border-b-[#db9abf] active:border-t-[#9b5a7f] active:border-l-[#9b5a7f]",
    steel: "active:border-r-[#abbcc7] active:border-b-[#abbcc7] active:border-t-[#6b7d87] active:border-l-[#6b7d87]",
    umber: "active:border-r-[#bca391] active:border-b-[#bca391] active:border-t-[#7e6655] active:border-l-[#7e6655]",
    lagoon: "active:border-r-[#95cad1] active:border-b-[#95cad1] active:border-t-[#598b92] active:border-l-[#598b92]",
};

const DirectAccessKeyForceDisabledButtonColors: Record<DirectAccessKeyColors, string> = {
    clay: "border-[#b86c51]! border!",
    blush: "border-[#b0928d]! border!",
    lilac: "border-[#a47499]! border!",
    mint: "border-[#80a799]! border!",
    // moss: "border-[#93a780]! border!",
    periwinkle: "border-[#8b80a7]! border!",
    taupe: "border-[#8c7c6b]! border!",
    orchid: "border-[#9b5a7f]! border!",
    steel: "border-[#6b7d87]! border!",
    umber: "border-[#7e6655]! border!",
    lagoon: "border-[#598b92]! border!",
};

export const ButtonColors: Record<ButtonColor, string> = {
    cyan: "bg-[#92e1fe] border-t-cyan-100 border-l-cyan-100 border-r-cyan-950 border-b-cyan-950",
    green: "bg-[#4b8747] border-t-green-200 border-l-green-200 border-r-green-950 border-b-green-950",
    gray: "bg-gray-300 border-t-gray-100 border-l-gray-100 border-r-gray-700 border-b-gray-700 border-3 outline outline-gray-700 -outline-offset-1",
    blue: "bg-blue-700 border-t-blue-300 border-l-blue-300 border-r-blue-900 border-b-blue-900 text-white",
    cornflower:
        "bg-[#5B95F9] border-t-blue-300 border-l-blue-300 border-r-blue-900 border-b-blue-900",
    emerald:
        "bg-[#05cf9c] border-t-green-200 border-l-green-200 border-r-green-950 border-b-green-950",
    red: "bg-red-500 border-t-red-200 border-l-red-200 border-r-red-900 border-b-red-900",
    salmon: "bg-red-400 border-t-red-200 border-l-red-200 border-r-red-900 border-b-red-900",
    peach: "bg-[#ffdf9e] border-t-orange-100 border-l-orange-100 border-r-yellow-600 border-b-yellow-600",
    honey: "bg-[#ffc246] border-t-orange-100 border-l-orange-100 border-r-yellow-700 border-b-yellow-700",
    sage: "bg-[#9bc997] border-t-[#b1d5ae] border-l-[#b1d5ae] border-r-[#2c3b2b] border-b-[#2c3b2b]",
    yellow: "bg-[#f8ec2c] border-t-yellow-100 border-l-yellow-100 border-r-[#aea51f] border-b-[#aea51f]",
    ...DirectAccessKeyButtonColors,
};

const ActiveButtonColors: Record<ButtonColor, string> = {
    cyan: "active:border-r-cyan-100 active:border-b-cyan-100 active:border-t-cyan-950 active:border-l-cyan-950",
    green: "active:border-r-green-200 active:border-b-green-200 active:border-t-green-950 active:border-l-green-950",
    gray: "active:border-r-gray-100 active:border-b-gray-100 active:border-t-gray-700 active:border-l-gray-700",
    blue: "active:border-r-blue-300 active:border-b-blue-300 active:border-t-blue-900 active:border-l-blue-900",
    cornflower:
        "active:border-r-blue-300 active:border-b-blue-300 active:border-t-blue-900 active:border-l-blue-900",
    emerald:
        "active:border-r-green-200 active:border-b-green-200 active:border-t-green-950 active:border-l-green-950",
    red: "active:border-r-red-200 active:border-b-red-200 active:border-t-red-900 active:border-l-red-900",
    salmon: "active:border-r-red-200 active:border-b-red-200 active:border-t-red-900 active:border-l-red-900",
    peach: "active:border-r-orange-100 active:border-b-orange-100 active:border-t-yellow-600 active:border-l-yellow-600",
    honey: "active:border-r-orange-100 active:border-b-orange-100 active:border-t-yellow-700 active:border-l-yellow-700",
    sage: "active:border-r-[#b1d5ae] active:border-b-[#b1d5ae] active:border-t-[#2c3b2b] active:border-l-[#2c3b2b]",
    yellow: "active:border-r-yellow-100 active:border-b-yellow-100 active:border-t-[#aea51f] active:border-l-[#aea51f]",
    ...DirectAccessKeyActiveButtonColors,
};

export const ForceDisabledButtonColors: Record<ButtonColor, string> = {
    cyan: "border-cyan-900! border!",
    green: "border-green-950! border!",
    gray: "border-gray-700! border! outline-none!",
    blue: "border-blue-950! border!",
    cornflower: "border-blue-950! border!",
    emerald: "border-emerald-950! border!",
    red: "border-red-950! border!",
    salmon: "border-red-950! border!",
    peach: "border-yellow-600! border!",
    honey: "border-yellow-700! border!",
    sage: "border-gray-700! border! outline-none!",
    yellow: "border-[#958e1a]! border!",
    ...DirectAccessKeyForceDisabledButtonColors,
};

export const ButtonHighlightColors: Record<ButtonHighlightColor, string> = {
    green: "bg-[#4b8747]",
    gray: "bg-gray-300",
};

function Button(props: ButtonProps) {
    const isTextChild = typeof props.children === "string" || typeof props.children === "number";

    const content = isTextChild ? (
        <p className="w-full text-center">{props.children}</p>
    ) : (
        props.children
    );

    return (
        <button
            className={clsx(
                "leading-5 w-20 border-2 rounded-md font-semibold cursor-pointer disabled:cursor-not-allowed",
                ButtonColors[props.color],
                ActiveButtonColors[props.color],
                (props.disabled || props.softDisabled) && ForceDisabledButtonColors[props.color],
                props.className,
                props.highlight !== undefined && "p-1.5",
                !props.disabled &&
                    !props.softDisabled &&
                    "active:*:translate-y-px active:*:translate-x-px",
            )}
            style={props.style}
            onClick={event => {
                if (props.muted !== true) {
                    void invokeSafe("audio_play_ui_click");
                }
                props.onClick?.(event);
            }}
            disabled={props.disabled}
            title={props.title}
        >
            {props.highlight === undefined ? (
                content
            ) : (
                <div
                    className={clsx(
                        "w-full h-full text-center flex flex-col items-center justify-center",
                        ButtonHighlightColors[props.highlight],
                    )}
                >
                    {content}
                </div>
            )}
        </button>
    );
}

export default Button;
