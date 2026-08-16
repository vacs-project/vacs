import {useBlinkStore} from "../../stores/blink-store";
import {someConnectionState, useCallStore} from "../../stores/call-store";
import {participantCount} from "../../types/call";
import Button from "./Button";

function ConferenceButton() {
    const blink = useBlinkStore(state => state.blink);
    const establishedCall = useCallStore(
        state =>
            state.callDisplay !== undefined &&
            state.callDisplay.type === "accepted" &&
            someConnectionState(state.callDisplay, "connected", true),
    );

    const conferenceState = useCallStore(state => state.conferenceState);
    const setConferenceState = useCallStore(state => state.actions.setConferenceState);

    const isConference = useCallStore(state => {
        const call = state.callDisplay?.call;
        if (call === undefined) return 0;
        return call.invitedTargets.length + participantCount(call.joinedParticipants) > 2;
    });
    const isConferenceLeader = useCallStore(state => state.callDisplay?.call.isConferenceLeader);

    const handleOnClick = () => {
        if (!establishedCall) return;

        if (conferenceState === "inactive" || conferenceState === "active") {
            setConferenceState("modify");
        } else {
            setConferenceState(isConference ? "active" : "inactive");
        }
    };

    return (
        <Button
            color={
                (blink && conferenceState === "modify") ||
                conferenceState === "active" ||
                (conferenceState === "inactive" && isConference)
                    ? "blue"
                    : "cyan"
            }
            onClick={handleOnClick}
            disabled={!establishedCall || isConferenceLeader === false}
            title="Conference Call"
        >
            CONF
        </Button>
    );
}

export default ConferenceButton;
