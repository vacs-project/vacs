import {ComponentChildren} from "preact";
import Button, {ButtonColor, ButtonHighlightColor} from "./Button.tsx";
import {clsx} from "clsx";
import {useSplitView} from "../../hooks/page-hook.ts";

type DirectAccessKeyProps = {
    color: ButtonColor;
    highlight?: ButtonHighlightColor;
    disabled?: boolean;
    className?: string;
    onClick?: () => void;
    children?: ComponentChildren;
};

function DirectAccessKeyButton(props: DirectAccessKeyProps) {
    const splitView = useSplitView();

    return (
        <Button
            color={props.color}
            highlight={props.highlight}
            disabled={props.disabled}
            className={clsx(
                "h-full rounded",
                props.color === "gray" ? "p-1.5" : "p-[calc(0.375rem+1px)]",
                props.className,
            )}
            style={{width: splitView ? "5.5rem" : "6.25rem"}}
            onClick={props.onClick}
        >
            {props.children}
        </Button>
    );
}

export default DirectAccessKeyButton;
