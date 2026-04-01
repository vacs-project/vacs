import {DirectAccessKey} from "../../types/profile.ts";
import Button, {ButtonColor} from "./Button.tsx";
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
    let realDefaultColor: ButtonColor | undefined;
    if (stationId !== undefined) {
        if (stationId.startsWith("2_TEST")) {
            realDefaultColor = "peach";
        } else if (stationId.startsWith("3_TEST")) {
            realDefaultColor = "honey";
        } else if (stationId.startsWith("4_TEST")) {
            realDefaultColor = "yellow";
        } else if (stationId.startsWith("5_TEST")) {
            realDefaultColor = "sage";
        } else if (stationId.startsWith("6_TEST")) {
            realDefaultColor = "green";
        } else if (stationId.startsWith("7_TEST")) {
            realDefaultColor = "red";
        } else {
            realDefaultColor = defaultColor;
        }
    }

    const {color, highlight, disabled, own, handleClick} = useStationKeyInteraction(
        stationId,
        realDefaultColor,
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
