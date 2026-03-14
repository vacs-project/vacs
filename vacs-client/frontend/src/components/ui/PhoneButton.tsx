import Button from "./Button.tsx";
import {useCallStore} from "../../stores/call-store.ts";
import {navigate} from "wouter/use-browser-location";
import {useFilterStore} from "../../stores/filter-store.ts";
import {useProfileStore, useProfileType} from "../../stores/profile-store.ts";
import {clsx} from "clsx";
import {useSettingsStore} from "../../stores/settings-store.ts";
import {getCallStateColors} from "../../utils/call-state-colors.ts";

function PhoneButton() {
    const blink = useCallStore(state => state.blink);
    const callDisplayType = useCallStore(state => state.callDisplay?.type);
    const enablePrio = useSettingsStore(state => state.callConfig.enablePriorityCalls);
    const outgoingPrio = useCallStore(state => state.callDisplay?.call.prio === true) && enablePrio;
    const incomingPrio =
        useCallStore(state => state.incomingCalls.some(call => call.prio)) && enablePrio;
    const incoming = useCallStore(state => state.incomingCalls.length > 0);
    const setFilter = useFilterStore(state => state.setFilter);
    const setSelectedPage = useProfileStore(state => state.setPage);
    const navigateParentPage = useProfileStore(state => state.navigateParentPage);

    const isTabbedProfile = useProfileType() === "tabbed";

    const {color, highlight} = getCallStateColors({
        inCall: callDisplayType === "accepted",
        isCalling: incoming && callDisplayType === undefined,
        beingCalled: callDisplayType === "outgoing",
        isRejected: callDisplayType === "rejected",
        isError: callDisplayType === "error",
        outgoingPrio,
        incomingPrio,
        blink,
    });

    return (
        <Button
            color={color}
            highlight={highlight}
            className={clsx(
                "min-h-16 text-lg transition-[width]",
                isTabbedProfile ? "w-24" : "w-46",
            )}
            onClick={() => {
                setFilter("");
                if (isTabbedProfile) {
                    navigateParentPage();
                } else {
                    setSelectedPage(undefined);
                }
                navigate("/");
            }}
        >
            Phone
        </Button>
    );
}

export default PhoneButton;
