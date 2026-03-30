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
    setClients: clients => {
        clients = [
            {positionId: "1", id: "10000001", displayName: "1", frequency: "199.998"},
            {positionId: "2", id: "10000001", displayName: "2", frequency: "199.998"},
            {positionId: "3", id: "10000001", displayName: "3", frequency: "199.998"},
            {positionId: "4", id: "10000001", displayName: "4", frequency: "199.998"},
            {positionId: "5", id: "10000001", displayName: "5", frequency: "199.998"},
            {positionId: "6", id: "10000001", displayName: "6", frequency: "199.998"},
            {positionId: "7", id: "10000001", displayName: "7", frequency: "199.998"},
            {positionId: "8", id: "10000001", displayName: "8", frequency: "199.998"},
            {positionId: "9", id: "10000001", displayName: "9", frequency: "199.998"},
            {positionId: "10", id: "10000001", displayName: "10", frequency: "199.998"},
            {positionId: "11", id: "10000001", displayName: "11", frequency: "199.998"},
            {positionId: "12", id: "10000001", displayName: "12", frequency: "199.998"},
            {positionId: "13", id: "10000001", displayName: "13", frequency: "199.998"},
            // {positionId: "14", id: "10000001", displayName: "14", frequency: "199.998"},
            // {positionId: "15", id: "10000001", displayName: "15", frequency: "199.998"},
            // {positionId: "16", id: "10000001", displayName: "16", frequency: "199.998"},
            // {positionId: "17", id: "10000001", displayName: "17", frequency: "199.998"},
            // {positionId: "18", id: "10000001", displayName: "18", frequency: "199.998"},
            // {positionId: "19", id: "10000001", displayName: "19", frequency: "199.998"},
            // {positionId: "20", id: "10000001", displayName: "20", frequency: "199.998"},
            // {positionId: "21", id: "10000001", displayName: "21", frequency: "199.998"},
            // {positionId: "22", id: "10000001", displayName: "22", frequency: "199.998"},
            // {positionId: "23", id: "10000001", displayName: "23", frequency: "199.998"},
            // {positionId: "24", id: "10000001", displayName: "24", frequency: "199.998"},
            // {positionId: "25", id: "10000001", displayName: "25", frequency: "199.998"},
            // {positionId: "26", id: "10000001", displayName: "26", frequency: "199.998"},
            // {positionId: "27", id: "10000001", displayName: "27", frequency: "199.998"},
            // {positionId: "28", id: "10000001", displayName: "28", frequency: "199.998"},
            // {positionId: "29", id: "10000001", displayName: "29", frequency: "199.998"},
            // {positionId: "30", id: "10000001", displayName: "30", frequency: "199.998"},
            // {positionId: "31", id: "10000001", displayName: "31", frequency: "199.998"},
            // {positionId: "32", id: "10000001", displayName: "32", frequency: "199.998"},
            // {positionId: "33", id: "10000001", displayName: "33", frequency: "199.998"},
            // {positionId: "34", id: "10000001", displayName: "34", frequency: "199.998"},
            // {positionId: "35", id: "10000001", displayName: "35", frequency: "199.998"},
            // {positionId: "36", id: "10000001", displayName: "36", frequency: "199.998"},
            // {positionId: "37", id: "10000001", displayName: "37", frequency: "199.998"},
            // {positionId: "38", id: "10000001", displayName: "38", frequency: "199.998"},
            // {positionId: "39", id: "10000001", displayName: "39", frequency: "199.998"},
            ...clients,
        ];

        set({clients});
    },
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
