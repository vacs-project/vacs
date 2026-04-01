import {DirectAccessKey} from "../../types/profile.ts";
import Button from "./Button.tsx";
import {clsx} from "clsx";
import ButtonLabel from "./ButtonLabel.tsx";
import {useStationKeyInteraction} from "../../hooks/station-key-interaction-hook.ts";

type DirectAccessStationKeyProps = {
    data: DirectAccessKey;
    className?: string;
};

function DirectAccessStationKey({
    data: {stationId, label, color: defaultColor},
    className,
}: DirectAccessStationKeyProps) {
    const {color, highlight, disabled, own, handleClick} = useStationKeyInteraction(
        stationId,
        defaultColor,
    );

    return (
        <Button
            color={color}
            highlight={highlight}
            disabled={disabled}
            className={clsx(
                className,
                "w-25 h-full rounded",
                (own || stationId === undefined) && "text-gray-500",
                color === "gray" ? "p-1.5" : "p-[calc(0.375rem+1px)]",
            )}
            onClick={handleClick}
        >
            <ButtonLabel label={label} />
        </Button>
    );
}

export default DirectAccessStationKey;
