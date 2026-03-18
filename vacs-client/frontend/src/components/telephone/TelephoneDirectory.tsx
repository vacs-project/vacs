import {useClientsStore} from "../../stores/clients-store.ts";
import {useMemo, useState} from "preact/hooks";
import {PositionId} from "../../types/generic.ts";
import List from "../ui/List.tsx";
import {clsx} from "clsx";
import Button from "../ui/Button.tsx";
import {useConnectionStore} from "../../stores/connection-store.ts";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {startCall, useCallStore} from "../../stores/call-store.ts";
import {TargetedEvent} from "preact";

function TelephoneDirectory() {
    const clients = useClientsStore(state => state.clients);
    const [filter, setFilter] = useState<string>("");

    const entries = useMemo(() => {
        const result = new Map<PositionId, string[]>();

        for (const client of clients) {
            if (
                client.positionId === undefined ||
                (filter !== "" &&
                    !client.positionId.toLowerCase().includes(filter.toLowerCase()) &&
                    !client.displayName.toLowerCase().includes(filter.toLowerCase()) &&
                    !client.id.toLowerCase().includes(filter.toLowerCase()))
            ) {
                continue;
            }

            const existing = result.get(client.positionId) ?? [];
            existing.push(`${client.displayName} (${client.id})`);
            result.set(client.positionId, existing);
        }

        return Array.from(result.entries())
            .map(([positionId, clients]) => ({positionId, clients}))
            .sort((a, b) => a.positionId.localeCompare(b.positionId));
    }, [clients, filter]);

    const [selectedEntry, setSelectedEntry] = useState<number>(0);
    const connected = useConnectionStore(state => state.connectionState === "connected");
    const callDisplay = useCallStore(state => state.callDisplay);

    const handleCallClick = useAsyncDebounce(async () => {
        const target = entries[selectedEntry]?.positionId;
        if (target === undefined || callDisplay !== undefined) return;
        await startCall({position: target});
    });

    const handleOnFilterChange = (e: TargetedEvent<HTMLInputElement>) => {
        if (!(e.target instanceof HTMLInputElement)) return;
        setFilter(e.target.value);
    };

    const positionRow = (index: number, isSelected: boolean, onClick: () => void) => {
        return <PositionRow position={entries[index]} isSelected={isSelected} onClick={onClick} />;
    };

    return (
        <div className="w-full h-full flex flex-col gap-3 p-3">
            <List
                className="w-full flex-1 min-h-0"
                itemsCount={entries.length}
                selectedItem={selectedEntry}
                setSelectedItem={setSelectedEntry}
                defaultRows={10}
                row={positionRow}
                header={[{title: "Position"}, {title: "Clients"}]}
                columnWidths={["1fr", "3fr"]}
                enableKeyboardNavigation={true}
            />
            <div className="h-15 w-full flex justify-between items-center pr-16">
                <div className="flex gap-2 items-center">
                    <p className="font-semibold">Search</p>
                    <input
                        type="text"
                        className={clsx(
                            "px-2 py-1.5 border border-gray-700 bg-yellow-50 font-semibold rounded focus:border-blue-500 focus:outline-none placeholder:text-gray-500",
                            "disabled:brightness-90 disabled:cursor-not-allowed",
                        )}
                        value={filter}
                        onChange={handleOnFilterChange}
                        onKeyDown={e => {
                            if (e.key === "Enter") {
                                e.currentTarget.blur();
                            }
                        }}
                    />
                </div>
                <Button
                    color="gray"
                    className="w-56 h-full text-xl"
                    disabled={!connected || entries[selectedEntry] === undefined}
                    onClick={handleCallClick}
                >
                    Call
                </Button>
            </div>
        </div>
    );
}

type PositionRowProps = {
    position: {positionId: PositionId; clients: string[]} | undefined;
    isSelected: boolean;
    onClick: () => void;
};

function PositionRow(props: PositionRowProps) {
    const color = props.isSelected ? "bg-blue-700 text-white" : "bg-yellow-50";

    return (
        <>
            <div
                className={clsx("px-0.5 flex items-center font-semibold", color)}
                onClick={props.onClick}
            >
                <p>{props.position?.positionId ?? ""}</p>
            </div>
            <div
                className={clsx("px-0.5 flex items-center font-semibold w-full truncate", color)}
                onClick={props.onClick}
                title={props.position?.clients.join(", ")}
            >
                {props.position?.clients.join(", ") ?? ""}
            </div>
        </>
    );
}

export default TelephoneDirectory;
