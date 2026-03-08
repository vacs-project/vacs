import Button from "./Button.tsx";
import {useCallStore} from "../../stores/call-store.ts";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {invokeSafe} from "../../error.ts";
import {useEffect, useState} from "preact/hooks";
import {listen} from "../../transport";
import {clsx} from "clsx";
import {useProfileType} from "../../stores/profile-store.ts";

function RadioPrioButton() {
    const [prio, setPrio] = useState<boolean>(false);
    const [implicitRadioPrio, setImplicitRadioPrio] = useState<boolean>(false);
    const callDisplayType = useCallStore(state => state.callDisplay?.type);

    const collapsed = useProfileType() === "tabbed";

    const handleOnClick = useAsyncDebounce(async () => {
        if (implicitRadioPrio) return;
        void invokeSafe("audio_set_radio_prio", {prio: !prio});
        setPrio(prio => !prio);
    });

    useEffect(() => {
        if (callDisplayType !== "accepted") {
            setPrio(false);
        }

        const unlistenPrio = listen<boolean>("audio:radio-prio", event => {
            setPrio(event.payload);
        });
        const unlistenImplicitPrio = listen<boolean>("audio:implicit-radio-prio", event => {
            setImplicitRadioPrio(event.payload);
        });

        return () => {
            unlistenPrio.then(fn => fn());
            unlistenImplicitPrio.then(fn => fn());
        };
    }, [callDisplayType]);

    return (
        <Button
            color={implicitRadioPrio || prio ? "blue" : "cyan"}
            className={clsx("text-lg transition-[width]", collapsed ? "w-38" : "w-46")}
            disabled={callDisplayType !== "accepted"}
            onClick={handleOnClick}
        >
            <p>
                RADIO
                <br />
                PRIO
            </p>
        </Button>
    );
}

export default RadioPrioButton;
