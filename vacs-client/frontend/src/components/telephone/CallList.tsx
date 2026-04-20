import Button from "../ui/Button.tsx";
import {CallListItem, useCallListArray, useCallListStore} from "../../stores/call-list-store.ts";
import {clsx} from "clsx";
import {startCall, useCallStore} from "../../stores/call-store.ts";
import {useState} from "preact/hooks";
import List from "../ui/List.tsx";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {invokeSafe} from "../../error.ts";
import {useConnectionStore} from "../../stores/connection-store.ts";
import incoming from "../../assets/call-list/incoming.svg";
import incomingX from "../../assets/call-list/incoming-x.svg";
import outgoing from "../../assets/call-list/outgoing.svg";
import outgoingX from "../../assets/call-list/outgoing-x.svg";

function CallList() {
    const calls = useCallListArray();
    const {clearCallList} = useCallListStore(state => state.actions);
    const callDisplay = useCallStore(state => state.callDisplay);
    const [selectedCall, setSelectedCall] = useState<number>(0);

    const connected = useConnectionStore(state => state.connectionState === "connected");

    const handleIgnoreClick = useAsyncDebounce(async () => {
        const peerId = calls[selectedCall]?.clientId;
        if (peerId === undefined || callDisplay !== undefined) return;
        await invokeSafe<boolean>("signaling_add_ignored_client", {clientId: peerId});
    });

    const handleCallClick = useAsyncDebounce(async () => {
        const target = calls[selectedCall]?.target;
        if (target === undefined || callDisplay !== undefined) return;
        await startCall(target);
    });

    const callRow = (index: number, isSelected: boolean, onClick: () => void) => {
        return <CallRow call={calls[index]} isSelected={isSelected} onClick={onClick} />;
    };

    return (
        <div className="w-[37.5rem] h-full flex flex-col gap-3 p-3">
            <List
                className="w-full"
                itemsCount={calls.length}
                selectedItem={selectedCall}
                setSelectedItem={setSelectedCall}
                defaultRows={10}
                row={callRow}
                header={[{title: "Name", className: "col-span-2"}, {title: "Number"}]}
                columnWidths={["minmax(3.5rem,auto)", "1fr", "1fr"]}
                enableKeyboardNavigation={true}
            />
            <div className="w-full shrink-0 flex flex-row justify-between pr-16 [&_button]:h-15 [&_button]:rounded">
                <Button color="gray" onClick={clearCallList}>
                    <p>
                        Delete
                        <br />
                        List
                    </p>
                </Button>
                <div className="flex gap-2">
                    <Button
                        color="gray"
                        disabled={calls[selectedCall]?.clientId === undefined}
                        onClick={handleIgnoreClick}
                    >
                        <p>
                            Ignore
                            <br />
                            CID
                        </p>
                    </Button>
                    <Button
                        color="gray"
                        className="w-56 text-xl"
                        disabled={!connected || calls[selectedCall]?.target === undefined}
                        onClick={handleCallClick}
                    >
                        Call
                    </Button>
                </div>
            </div>
        </div>
    );
}

type CallRowProps = {
    call: CallListItem | undefined;
    isSelected: boolean;
    onClick: () => void;
};

function CallRow(props: CallRowProps) {
    const color = props.isSelected ? "bg-blue-700 text-white" : "bg-yellow-50";

    return (
        <>
            <div
                className={clsx(
                    "p-0.5 text-center flex flex-col justify-between items-center h-0 min-h-full",
                    color,
                )}
                onClick={props.onClick}
            >
                <CallRowStatus call={props.call} />
                <p className="tracking-wider font-semibold leading-3.5 pb-px">
                    {props.call?.time ?? ""}
                </p>
            </div>
            <div
                className={clsx("px-0.5 flex items-center font-semibold", color)}
                onClick={props.onClick}
            >
                {props.call?.name ?? ""}
            </div>
            <div
                className={clsx("px-0.5 flex items-center font-semibold", color)}
                onClick={props.onClick}
            >
                {props.call?.clientId ?? ""}
            </div>
        </>
    );
}

function CallRowStatus({call}: {call: CallListItem | undefined}) {
    if (call === undefined) return <></>;

    const status =
        call.type === "IN"
            ? call.answered !== false
                ? [incoming, "IN", "Incoming"]
                : [incomingX, "IN X", "Incoming - Unanswered"]
            : call.answered !== false
              ? [outgoing, "OUT", "Outgoing"]
              : [outgoingX, "OUT X", "Outgoing - Unanswered"];

    return (
        <img src={status[0]} alt={status[1]} title={status[2]} className="flex-1 max-h-6 min-h-4" />
    );
}

export default CallList;
