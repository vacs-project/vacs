export type RadioState = {
    state:
        | "NotConfigured"
        | "Disconnected"
        | "Connected"
        | "VoiceConnected"
        | "RxIdle"
        | "RxActive"
        | "TxActive"
        | "Error";
    data?: number[];
};

export type RadioStation = {
    callsign?: string;
    frequency: number;
    rx: boolean;
    tx: boolean;
    xc: boolean;
    xca: boolean;
    headset: boolean;
    output_muted: boolean;
    is_available: boolean;
};

export type StationStateUpdate = {
    rx?: boolean;
    tx?: boolean;
    xca?: boolean;
    headset?: boolean;
    output_muted?: boolean;
};
