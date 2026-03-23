export type CallConfig = {
    highlightIncomingCallTarget: boolean;
    enablePriorityCalls: boolean;
    enableCallStartSound: boolean;
    enableCallEndSound: boolean;
    useDefaultCallSources: boolean;
};

export type RemoteConfig = {
    enabled: boolean;
    listenAddr: string;
};

export type RemoteStatus = {
    listening: boolean;
    connectedClients: number;
};

export type ClockMode = "Realtime" | "Relaxed" | "Day";
