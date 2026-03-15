import {ClientId, PositionId, StationId} from "./generic.ts";
import {SessionProfile} from "./profile.ts";

export type ClientInfo = {
    id: ClientId;
    positionId: PositionId | undefined;
    displayName: string;
    frequency: string;
};

export type SessionInfo = {
    client: ClientInfo;
    profile: SessionProfile;
    defaultCallSources: StationId[];
};

export function splitDisplayName(name: string): [string, string] {
    const parts = name.replaceAll("-", "_").split("_");

    if (parts.length <= 1) {
        return [parts[0], ""];
    }

    return [parts.slice(0, parts.length - 1).join(" "), parts[parts.length - 1]];
}

export type ClientPageSettings = {
    selected?: string;
    configs: Record<string, ClientPageConfig>;
};

export type ClientPageConfig = {
    include?: string[];
    exclude?: string[];
    priority?: string[];
    frequencies: FrequencyDisplayMode;
    grouping: ClientGroupMode;
};

export type FrequencyDisplayMode = "ShowAll" | "HideAll";
export type ClientGroupMode = "None" | "Fir" | "FirAndIcao" | "Icao";

function globToRegex(pattern: string): RegExp {
    const escaped = pattern
        .replace(/[.+^${}()|[\]\\]/g, "\\$&") // Escape regex special chars except * and ?
        .replace(/\*/g, ".*") // * matches any characters
        .replace(/\?/g, "."); // ? matches single character

    return new RegExp(`^${escaped}$`, "i");
}

function matchesAnyPattern(callsign: string, patterns: string[] | undefined): boolean {
    if (patterns === undefined || patterns.length === 0) return false;
    return patterns.some(pattern => globToRegex(pattern).test(callsign));
}

function findFirstMatchIndex(callsign: string, patterns: string[] | undefined): number {
    if (patterns === undefined || patterns.length === 0) return -1;
    return patterns.findIndex(pattern => globToRegex(pattern).test(callsign));
}

function filterClients(clients: ClientInfo[], config: ClientPageConfig | undefined): ClientInfo[] {
    if (!config) return clients;

    return clients.filter(client => {
        if (matchesAnyPattern(client.displayName, config.exclude)) return false;
        if ((config.include?.length ?? 0) === 0) return true;
        return matchesAnyPattern(client.displayName, config.include);
    });
}

function sortClients(clients: ClientInfo[], config: ClientPageConfig | undefined): ClientInfo[] {
    if (!config) return clients;

    return clients.sort((a, b) => {
        const aPriorityIndex = findFirstMatchIndex(a.displayName, config.priority);
        const bPriorityIndex = findFirstMatchIndex(b.displayName, config.priority);

        // 1. Sort by priority bucket (lower index = higher priority)
        const aEffectivePriority = aPriorityIndex === -1 ? Number.MAX_SAFE_INTEGER : aPriorityIndex;
        const bEffectivePriority = bPriorityIndex === -1 ? Number.MAX_SAFE_INTEGER : bPriorityIndex;

        if (aEffectivePriority !== bEffectivePriority) {
            return aEffectivePriority - bEffectivePriority;
        }

        const [aStationName, aStationType] = splitDisplayName(a.displayName);
        const [bStationName, bStationType] = splitDisplayName(b.displayName);

        // 2. Sort non-prioritized station types before clients without any station type
        if (aStationType.length === 0 && bStationType.length > 0) {
            return 1;
        } else if (aStationType.length > 0 && bStationType.length === 0) {
            return -1;
        }

        // 3. Sort by station type alphabetically
        const stationType = aStationType.localeCompare(bStationType);

        // 4. Sort by station name alphabetically
        return stationType !== 0 ? stationType : aStationName.localeCompare(bStationName);
    });
}

export function filterAndSortClients(clients: ClientInfo[], config: ClientPageConfig | undefined) {
    const filtered = filterClients(clients, config);
    return sortClients(filtered, config);
}
