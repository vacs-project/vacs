import {clsx} from "clsx";

export type Status = "green" | "yellow" | "red" | "gray" | "blue";

export const StatusColors: Record<Status, string> = {
    green: "bg-green-600 border-green-700",
    yellow: "bg-yellow-500 border-yellow-600",
    red: "bg-red-400 border-red-700",
    gray: "bg-gray-400 border-gray-600",
    blue: "bg-blue-500 border-blue-600",
};

type StatusIndicatorProps = {
    status: Status;
    className?: string;
    title?: string;
    onClick?: () => void;
};

function StatusIndicator(props: StatusIndicatorProps) {
    return (
        <div
            className={clsx(
                "shrink-0 h-3 w-3 rounded-full border",
                StatusColors[props.status],
                props.className,
            )}
            title={props.title}
            onClick={props.onClick}
        ></div>
    );
}

export default StatusIndicator;
