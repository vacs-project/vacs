import {DirectAccessKey} from "../../types/profile.ts";
import Button from "./Button.tsx";
import {clsx} from "clsx";
import ButtonLabel from "./ButtonLabel.tsx";
import {useDirectAccessStationKey} from "../../hooks/use-direct-access-station-key.ts";

type DirectAccessStationKeyProps = {
    data: DirectAccessKey;
    className?: string;
};

function DirectAccessStationKey({
    data: {stationId, label},
    className,
}: DirectAccessStationKeyProps) {
    const {color, highlight, handleClick, disabled, own, hasStationId} = useDirectAccessStationKey({
        stationId,
    });

    return (
        <Button
            color={color}
            highlight={highlight}
            disabled={disabled}
            className={clsx(
                className,
                "w-25 h-full rounded",
                (own || !hasStationId) && "text-gray-500",
                color === "gray" ? "p-1.5" : "p-[calc(0.375rem+1px)]",
            )}
            onClick={handleClick}
        >
            <ButtonLabel label={label} />
        </Button>
    );
}

export default DirectAccessStationKey;
