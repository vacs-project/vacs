import {clsx} from "clsx";
import ButtonLabel from "./ButtonLabel.tsx";
import {keyStationIds, useStationKeyInteraction} from "../../hooks/station-key-interaction-hook.ts";
import DirectAccessKeyButton from "./DirectAccessKeyButton.tsx";
import {DirectAccessKey} from "../../types/profile.ts";

type DirectAccessStationsKeyProps = {
    data: DirectAccessKey;
    className?: string;
};

function DirectAccessStationsKey({data, className}: DirectAccessStationsKeyProps) {
    const stationIds = keyStationIds(data);
    const {color, highlight, disabled, own, handleClick} = useStationKeyInteraction(
        stationIds,
        data.color,
    );

    return (
        <DirectAccessKeyButton
            color={color}
            highlight={highlight}
            disabled={disabled}
            className={clsx((own || stationIds.length === 0) && "text-gray-500", className)}
            onClick={handleClick}
        >
            <ButtonLabel label={data.label} />
        </DirectAccessKeyButton>
    );
}

export default DirectAccessStationsKey;
