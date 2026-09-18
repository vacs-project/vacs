import {clsx} from "clsx";
import {invokeStrict} from "../../error.ts";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {useCallStore} from "../../stores/call-store.ts";
import {useFilterStore} from "../../stores/filter-store.ts";
import {goToPage} from "../../stores/navigation-store.ts";
import {useProfileStore, useProfileType} from "../../stores/profile-store.ts";
import Button from "./Button.tsx";
import {useSplitView} from "../../hooks/page-hook.ts";

function EndButton() {
    const callDisplay = useCallStore(state => state.callDisplay);
    const {endCall, dismissRejectedCall, dismissErrorCall} = useCallStore(state => state.actions);
    const setFilter = useFilterStore(state => state.setFilter);
    const setSelectedPage = useProfileStore(state => state.setPage);
    const navigateParentPage = useProfileStore(state => state.navigateParentPage);
    const splitView = useSplitView();

    const isTabbedProfile = useProfileType() === "tabbed";

    const endAnyCall = useAsyncDebounce(async () => {
        if (callDisplay?.type === "accepted" || callDisplay?.type === "outgoing") {
            try {
                await invokeStrict("signaling_end_call", {callId: callDisplay.call.callId});
                endCall();
            } catch {}
        } else if (callDisplay?.type === "rejected") {
            dismissRejectedCall();
        } else if (callDisplay?.type === "error") {
            dismissErrorCall();
        }
    });

    const handleOnClick = async () => {
        setFilter("");
        if (isTabbedProfile) {
            navigateParentPage();
        } else {
            setSelectedPage(undefined);
        }
        goToPage(splitView ? "split" : "phone");

        void endAnyCall();
    };

    return (
        <Button
            color="cyan"
            className={clsx("text-xl transition-[width]", isTabbedProfile ? "w-20" : "w-44 px-10")}
            onClick={handleOnClick}
        >
            END
        </Button>
    );
}

export default EndButton;
