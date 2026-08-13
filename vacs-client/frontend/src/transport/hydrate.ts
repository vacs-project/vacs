import type {ClientId, StationId} from "../types/generic.ts";
import type {ClientInfo, ClientPageSettings, SessionInfo} from "../types/client.ts";
import type {StationInfo} from "../types/station.ts";
import type {CallConfig} from "../types/settings.ts";
import type {Capabilities} from "../types/capabilities.ts";
import {type SignalingConnectionState, useConnectionStore} from "../stores/connection-store.ts";
import {useAuthStore} from "../stores/auth-store.ts";
import {useClientsStore} from "../stores/clients-store.ts";
import {useStationsStore} from "../stores/stations-store.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import {useCapabilitiesStore} from "../stores/capabilities-store.ts";
import {useProfileStore} from "../stores/profile-store.ts";
import {withSyncSuppressed} from "./store-sync.ts";

export type SessionStateSnapshot = {
    connectionState: SignalingConnectionState;
    sessionInfo: SessionInfo | null;
    defaultCallSources: StationId[];
    stations: StationInfo[];
    clients: ClientInfo[];
    clientId: ClientId | null;
    callConfig: CallConfig;
    clientPageSettings: ClientPageSettings;
    capabilities: Capabilities;
};

export function hydrateStores(snapshot: SessionStateSnapshot) {
    // Suppress sync re-broadcast: the snapshot's values came from the desktop,
    // but at this point our stores still hold local defaults for everything the
    // snapshot doesn't cover (transmitConfig, playbackEnabled, ...). Echoing a
    // partially-default state back would clobber the desktop's stores.
    withSyncSuppressed(() => applySnapshot(snapshot));
    console.log("[remote] Stores hydrated from session state snapshot");
}

function applySnapshot(snapshot: SessionStateSnapshot) {
    const {setConnectionInfo, setConnectionState} = useConnectionStore.getState();
    const {setAuthenticated, setUnauthenticated} = useAuthStore.getState();
    const {setClients} = useClientsStore.getState();
    const {setStations, setPositionDefaultSources} = useStationsStore.getState();
    const {setCallConfig, setClientPageSettings} = useSettingsStore.getState();
    const {setCapabilities} = useCapabilitiesStore.getState();
    const {setProfile} = useProfileStore.getState();

    setConnectionState(snapshot.connectionState);
    if (snapshot.sessionInfo) {
        setConnectionInfo(snapshot.sessionInfo.client);
    }

    if (snapshot.clientId) {
        setAuthenticated(snapshot.clientId);
    } else {
        setUnauthenticated();
    }

    setStations(snapshot.stations);
    setPositionDefaultSources(snapshot.defaultCallSources);
    setClients(snapshot.clients);

    if (
        snapshot.sessionInfo?.profile.type === "changed" &&
        snapshot.sessionInfo.profile.activeProfile?.profile
    ) {
        setProfile(snapshot.sessionInfo.profile.activeProfile.profile);
    }

    setCallConfig(snapshot.callConfig);
    setClientPageSettings(snapshot.clientPageSettings);

    setCapabilities(snapshot.capabilities);
}
