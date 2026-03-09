import {ClientInfo} from "../types/client.ts";
import {create} from "zustand/react";
import {ClientId} from "../types/generic.ts";

type ClientsState = {
    clients: ClientInfo[];
    setClients: (clients: ClientInfo[]) => void;
    addClient: (client: ClientInfo) => void;
    getClientInfo: (cid: ClientId) => ClientInfo;
    removeClient: (cid: ClientId) => void;
};

export const useClientsStore = create<ClientsState>()((set, get) => ({
    clients: [],
    setClients: clients => set({clients}),
    addClient: client => {
        const clients = get().clients.filter(c => c.id !== client.id);
        clients.push(client);
        set({clients});
    },
    getClientInfo: cid => {
        const client = get().clients.find(c => c.id === cid);
        if (client === undefined) {
            return {
                id: cid,
                displayName: cid,
                positionId: undefined,
                alias: undefined,
                frequency: "",
            };
        }
        return client;
    },
    removeClient: cid => {
        set({
            clients: get().clients.filter(client => client.id !== cid),
        });
    },
}));
