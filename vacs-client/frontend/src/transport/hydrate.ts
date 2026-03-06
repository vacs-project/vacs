import type {ClientId} from "../types/generic.ts";
import type {ClientInfo, ClientPageSettings, SessionInfo} from "../types/client.ts";
import type {StationInfo} from "../types/station.ts";
import type {CallConfig} from "../types/settings.ts";
import type {Capabilities} from "../types/capabilities.ts";
import type {ConnectionState} from "../stores/call-store.ts";
import {useConnectionStore} from "../stores/connection-store.ts";
import {useAuthStore} from "../stores/auth-store.ts";
import {useClientsStore} from "../stores/clients-store.ts";
import {useStationsStore} from "../stores/stations-store.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import {useCapabilitiesStore} from "../stores/capabilities-store.ts";
import {useProfileStore} from "../stores/profile-store.ts";

export type SessionStateSnapshot = {
    connectionState: ConnectionState;
    sessionInfo: SessionInfo | null;
    stations: StationInfo[];
    clients: ClientInfo[];
    clientId: ClientId | null;
    callConfig: CallConfig;
    clientPageSettings: ClientPageSettings;
    capabilities: Capabilities;
};

export function hydrateStores(snapshot: SessionStateSnapshot) {
    const {setConnectionInfo, setConnectionState} = useConnectionStore.getState();
    const {setAuthenticated, setUnauthenticated} = useAuthStore.getState();
    const {setClients} = useClientsStore.getState();
    const {setStations} = useStationsStore.getState();
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

    console.log("[remote] Stores hydrated from session state snapshot");
}
