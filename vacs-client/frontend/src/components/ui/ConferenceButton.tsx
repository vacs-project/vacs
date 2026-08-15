import {useBlinkStore} from "../../stores/blink-store";
import {someConnectionState, useCallStore} from "../../stores/call-store";
import Button from "./Button";

function ConferenceButton() {
    const blink = useBlinkStore(state => state.blink);
    const establishedCall = useCallStore(
        state =>
            state.callDisplay !== undefined &&
            state.callDisplay.type === "accepted" &&
            someConnectionState(state.callDisplay, "connected"),
    );
    const conferenceState = useCallStore(state => state.conferenceState);
    const setConferenceState = useCallStore(state => state.actions.setConferenceState);

    const handleOnClick = () => {
        if (!establishedCall) return;

        if (conferenceState === "inactive" || conferenceState === "active") {
            setConferenceState("modify");
        } else {
            setConferenceState("inactive"); // TODO: needs to be the prev value (more precisely the actual conference state of the call display)
        }
    };

    return (
        <Button
            color={
                (blink && conferenceState === "modify") || conferenceState === "active"
                    ? "blue"
                    : "cyan"
            }
            onClick={handleOnClick}
            disabled={!establishedCall}
            title="Conference Call"
        >
            CONF
        </Button>
    );
}

export default ConferenceButton;
