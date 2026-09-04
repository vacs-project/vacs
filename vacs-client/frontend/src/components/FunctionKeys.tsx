import mission from "../assets/mission.svg";
import wrenchAndDriver from "../assets/wrench-and-driver.svg";
import {useCallStore} from "../stores/call-store.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import Button from "./ui/Button.tsx";
import ConferenceButton from "./ui/ConferenceButton.tsx";
import LinkButton from "./ui/LinkButton.tsx";

function FunctionKeys() {
    const prio = useCallStore(state => state.prio);
    const setPrio = useCallStore(state => state.actions.setPrio);
    const disablePrio = useSettingsStore(state => !state.callConfig.enablePriorityCalls);

    return (
        <div className="h-20 w-full flex flex-row gap-2 justify-between p-2 [&>button]:shrink-0">
            <Button
                color={prio ? "blue" : "cyan"}
                onClick={() => setPrio(!prio)}
                disabled={disablePrio}
            >
                PRIO
            </Button>
            <Button color="cyan" className="text-slate-400" disabled={true}>
                HOLD
            </Button>
            <ConferenceButton />
            <Button color="cyan" className="text-slate-400" disabled={true}>
                <p>
                    SUITE
                    <br />
                    PICKUP
                </p>
            </Button>
            <Button color="cyan" className="text-slate-400" disabled={true}>
                TRANS
            </Button>
            <Button color="cyan" className="text-slate-400" disabled={true}>
                DIV
            </Button>
            <LinkButton menu="playback" className="h-full">
                <p>
                    PLAY
                    <br />
                    BACK
                </p>
            </LinkButton>
            <Button color="cyan" className="text-slate-400" disabled={true}>
                <p>
                    PLC
                    <br />
                    LSP
                    <br />
                    on/off
                </p>
            </Button>
            <Button color="cyan" className="text-slate-400" disabled={true}>
                SPLIT
            </Button>
            <LinkButton menu="settings" className="h-full">
                <img src={wrenchAndDriver} alt="Settings" className="h-12 w-12" draggable={false} />
            </LinkButton>
            <LinkButton menu="mission" className="h-full">
                <img src={mission} alt="Mission" className="h-14 w-14" draggable={false} />
            </LinkButton>
        </div>
    );
}

export default FunctionKeys;
