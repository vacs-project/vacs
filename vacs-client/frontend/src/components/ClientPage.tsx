import {useClientsStore} from "../stores/clients-store.ts";
import DirectAccessClientKey from "./ui/DirectAccessClientKey.tsx";
import {useMemo} from "preact/hooks";
import {ClientInfo, ClientPageConfig, filterAndSortClients} from "../types/client.ts";
import {useFilterStore} from "../stores/filter-store.ts";
import Button from "./ui/Button.tsx";
import {useCallStore} from "../stores/call-store.ts";
import {clsx} from "clsx";
import {useBlinkStore} from "../stores/blink-store.ts";

type ClientPageProps = {
    config: ClientPageConfig;
};

function ClientPage({config}: ClientPageProps) {
    const allClients = useClientsStore(state => state.clients);
    const {filter, setFilter} = useFilterStore();

    const clients = useMemo(() => {
        return filterAndSortClients(allClients, config);
    }, [allClients, config]);

    const getGroups = (clients: ClientInfo[], slice: number, prefix = "") => {
        const groups = [
            ...clients
                .filter(
                    client =>
                        client.displayName.startsWith(prefix) && client.displayName.includes("_"),
                )
                .reduce<
                    Set<string>
                >((acc, val) => acc.add(val.displayName.split("_")[0].slice(0, slice)), new Set([])),
        ];

        if (
            clients.find(client => !client.displayName.includes("_")) !== undefined &&
            prefix === ""
        ) {
            groups.push("OTHER");
        }

        return groups;
    };

    const renderClients = (clients: ClientInfo[]) => {
        return clients.map((client, index) => (
            <DirectAccessClientKey key={index} client={client} config={config} />
        ));
    };

    const renderGroups = (groups: string[]) => {
        return groups.map((group, index) => (
            <ClientPageGroupKey key={index} group={group} setFilter={setFilter} />
        ));
    };

    if (filter === "OTHER") {
        return renderClients(clients.filter(client => !client.displayName.includes("_")));
    }

    switch (config.grouping) {
        case "Fir":
        case "Icao": {
            if (filter !== "") {
                return renderClients(
                    clients.filter(client => client.displayName.startsWith(filter)),
                );
            }

            const slice = config.grouping === "Fir" ? 2 : 4;
            return renderGroups(getGroups(clients, slice));
        }
        case "FirAndIcao": {
            if (filter === "") {
                return renderGroups(getGroups(clients, 2));
            } else if (filter.length === 2) {
                return renderGroups(getGroups(clients, 4, filter));
            }
            return renderClients(clients.filter(client => client.displayName.startsWith(filter)));
        }
        case undefined:
        case "None":
        default:
            return renderClients(clients);
    }
}

function ClientPageGroupKey({
    group,
    setFilter,
}: {
    group: string;
    setFilter: (filter: string) => void;
}) {
    const clients = useClientsStore(state => state.clients);
    const blink = useBlinkStore(state => state.blink);
    const callDisplay = useCallStore(state => state.callDisplay);
    const incomingCalls = useCallStore(state => state.incomingCalls);

    const clientIdsInGroup = useMemo(() => {
        return clients
            .filter(
                client =>
                    client.displayName.startsWith(group) ||
                    (group === "OTHER" && !client.displayName.includes("_")),
            )
            .map(client => client.id);
    }, [clients, group]);

    const isCalling = incomingCalls.some(call => clientIdsInGroup.includes(call.source.clientId));
    const involved =
        callDisplay !== undefined &&
        (clientIdsInGroup.includes(callDisplay.call.source.clientId) ||
            (callDisplay.call.target.client !== undefined &&
                clientIdsInGroup.includes(callDisplay.call.target.client)));
    const beingCalled = callDisplay?.type === "outgoing" && involved;
    const inCall = callDisplay?.type === "accepted" && involved;
    const isRejected = callDisplay?.type === "rejected" && involved;
    const isError = callDisplay?.type === "error" && involved;

    const color = inCall
        ? "green"
        : (isCalling || isRejected) && blink
          ? "green"
          : isError && blink
            ? "red"
            : "gray";

    return (
        <Button
            color={color}
            highlight={beingCalled || isRejected ? "green" : undefined}
            className={clsx(
                "w-25 h-full rounded leading-4.5!",
                color === "gray" ? "p-1.5" : "p-[calc(0.375rem+1px)]",
            )}
            onClick={() => setFilter(group)}
        >
            <p className="w-full truncate leading-3.5" title={group}>
                {group}
                <br />
                ...
            </p>
        </Button>
    );
}

export default ClientPage;
