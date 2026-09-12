export type CallConfig = {
    highlightIncomingCallTarget: boolean;
    enablePriorityCalls: boolean;
    enableCallStartSound: boolean;
    enableCallEndSound: boolean;
    enableParticipantJoinedSound: boolean;
    enableParticipantLeftSound: boolean;
    useDefaultCallSources: boolean;
    forceRelay: boolean;
};

export type RemoteConfig = {
    enabled: boolean;
    listenAddr: string;
    // Not exposed in the UI, but required by the backend's RemoteConfig schema;
    // it must round-trip through remote_get_config -> remote_set_config intact.
    serveFrontend: boolean;
};

export type RemoteStatus = {
    listening: boolean;
    connectedClients: number;
};

export type ClockMode = "Realtime" | "Relaxed" | "Day";

export const ALL_CPL_MODES = ["Original", "Fast"] as const;
export type CplMode = (typeof ALL_CPL_MODES)[number];

export function isCplMode(value: string): value is CplMode {
    return ALL_CPL_MODES.includes(value as CplMode);
}
