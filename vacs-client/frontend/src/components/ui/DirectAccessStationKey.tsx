import {clsx} from "clsx";
import ButtonLabel from "./ButtonLabel.tsx";
import {useStationKeyInteraction} from "../../hooks/station-key-interaction-hook.ts";
import DirectAccessKeyButton from "./DirectAccessKeyButton.tsx";
import {DirectAccessKey} from "../../types/profile.ts";

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
        <DirectAccessKeyButton
            color={color}
            highlight={highlight}
            disabled={disabled}
            className={clsx((own || stationId === undefined) && "text-gray-500", className)}
            onClick={handleClick}
        >
            <ButtonLabel label={label} />
        </DirectAccessKeyButton>
    );
}

export default DirectAccessStationKey;
