import {clsx} from "clsx";
import {useBlinkStore} from "../../stores/blink-store.ts";
import {useCallStore} from "../../stores/call-store.ts";
import {useFilterStore} from "../../stores/filter-store.ts";
import {goToPage} from "../../stores/navigation-store.ts";
import {useProfileStore, useProfileType} from "../../stores/profile-store.ts";
import {useSettingsStore} from "../../stores/settings-store.ts";
import {getCallStateColors} from "../../utils/call-state-colors.ts";
import Button from "./Button.tsx";

function PhoneButton() {
    const blink = useBlinkStore(state => state.blink);
    const callDisplayType = useCallStore(state => state.callDisplay?.type);
    const enablePrio = useSettingsStore(state => state.callConfig.enablePriorityCalls);
    const prio = useCallStore(
        state =>
            enablePrio &&
            (state.callDisplay !== undefined
                ? state.callDisplay.prioTargets.length > 0
                : state.incomingCalls.some(call => call.prio)),
    );
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
        prio,
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
                goToPage("phone");
            }}
        >
            Phone
        </Button>
    );
}

export default PhoneButton;
