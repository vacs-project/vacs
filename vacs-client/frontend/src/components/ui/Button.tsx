import {clsx} from "clsx";
import {ComponentChildren, CSSProperties} from "preact";
import {invokeSafe} from "../../error.ts";
import {
    CustomActiveButtonColors,
    CustomButtonColor,
    CustomButtonColors,
    CustomButtonHighlightColors,
    CustomForceDisabledButtonColors,
} from "../../types/custom-button-colors.ts";

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
    | CustomButtonColor;
export type ButtonHighlightColor = "green" | "gray" | CustomButtonColor;

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
    ...CustomButtonColors,
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
    ...CustomActiveButtonColors,
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
    ...CustomForceDisabledButtonColors,
};

export const ButtonHighlightColors: Record<ButtonHighlightColor, string> = {
    green: "bg-[#4b8747]",
    gray: "bg-gray-300",
    ...CustomButtonHighlightColors,
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
