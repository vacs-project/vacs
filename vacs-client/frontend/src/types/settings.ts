export type CallConfig = {
    highlightIncomingCallTarget: boolean;
    enablePriorityCalls: boolean;
    enableCallStartSound: boolean;
    enableCallEndSound: boolean;
};

export type RemoteConfig = {
    enabled: boolean;
    listenAddr: string;
};

export type RemoteStatus = {
    listening: boolean;
    connectedClients: number;
};
