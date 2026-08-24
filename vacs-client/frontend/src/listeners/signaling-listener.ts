import {IncomingCallListEntry, useCallListStore} from "../stores/call-list-store.ts";
import {useCallStore} from "../stores/call-store.ts";
import {useClientsStore} from "../stores/clients-store.ts";
import {useConnectionStore} from "../stores/connection-store.ts";
import {useErrorOverlayStore} from "../stores/error-overlay-store.ts";
import {useFilterStore} from "../stores/filter-store.ts";
import {goToPage} from "../stores/navigation-store.ts";
import {useProfileStore} from "../stores/profile-store.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import {useStationsStore} from "../stores/stations-store.ts";
import {listen, UnlistenFn} from "../transport";
import {Call, CallTarget, CallUpdate} from "../types/call.ts";
import {ClientInfo, ClientPageSettings, SessionInfo} from "../types/client.ts";
import {CallId, ClientId, PositionId} from "../types/generic.ts";
import {Profile} from "../types/profile.ts";
import {StationChange, StationInfo} from "../types/station.ts";

export function setupSignalingListeners() {
    const {setClients, addClient, removeClient} = useClientsStore.getState();
    const {
        setStations,
        addStationChanges,
        setPositionDefaultSources,
        reset: resetStationsStore,
    } = useStationsStore.getState();
    const {
        addIncomingCall,
        updateCall,
        removeCall,
        rejectTargets,
        acceptIncomingCall,
        setMaxConferenceSize,
        reset: resetCallStore,
    } = useCallStore.getState().actions;
    const {addIncomingCallListEntry, clearCallList} = useCallListStore.getState().actions;
    const {setConnectionState, setConnectionInfo, setPositionsToSelect} =
        useConnectionStore.getState();
    const {setProfile, reset: resetProfileStore} = useProfileStore.getState();
    const {open: openErrorOverlay, closeIfTitle: closeErrorOverlayIfTitle} =
        useErrorOverlayStore.getState();
    const {setFilter} = useFilterStore.getState();
    const {setClientPageSettings} = useSettingsStore.getState();

    const unlistenFns: Promise<UnlistenFn>[] = [];

    const init = () => {
        unlistenFns.push(
            listen<SessionInfo>("signaling:connected", event => {
                setConnectionState("connected");
                setConnectionInfo(event.payload.client);
                if (
                    event.payload.profile.type === "changed" &&
                    event.payload.profile.activeProfile !== undefined &&
                    event.payload.profile.activeProfile.profile !== undefined
                ) {
                    setProfile(event.payload.profile.activeProfile.profile);
                }
                setPositionDefaultSources(event.payload.defaultCallSources);
                setMaxConferenceSize(event.payload.maxConfSize);
            }),
            listen("signaling:reconnecting", () => {
                setConnectionState("connecting");
            }),
            listen("signaling:disconnected", () => {
                setConnectionState("disconnected");
                setConnectionInfo({displayName: "", positionId: undefined, frequency: ""});
                setClients([]);
                resetStationsStore();
                resetCallStore();
                clearCallList();
                resetProfileStore();
                setFilter("");
            }),
            listen<PositionId[]>("signaling:ambiguous-position", event => {
                setConnectionState("connecting");
                setPositionsToSelect(event.payload);
            }),
            listen<StationInfo[]>("signaling:station-list", event => {
                setStations(event.payload);
            }),
            listen<StationChange[]>("signaling:station-changes", event => {
                addStationChanges(event.payload);
            }),
            listen<ClientInfo[]>("signaling:client-list", event => {
                setClients(event.payload);
            }),
            listen<ClientInfo>("signaling:client-connected", event => {
                addClient(event.payload);
            }),
            listen<ClientId>("signaling:client-disconnected", event => {
                removeClient(event.payload);
            }),
            listen<ClientId>("signaling:client-not-found", event => {
                removeClient(event.payload);
                openErrorOverlay(
                    "Client not found",
                    `Server cannot find a client with CID ${event.payload}`,
                    false,
                    5000,
                );
            }),
            listen<Call>("signaling:call-invitation", event => {
                addIncomingCall(event.payload);
            }),
            listen<CallId>("signaling:accept-incoming-call", event => {
                acceptIncomingCall(event.payload);
            }),
            listen<CallUpdate>("signaling:call-update", event => {
                updateCall(event.payload);
            }),
            listen<CallId>("signaling:call-end", event => {
                removeCall(event.payload, true);
            }),
            listen<CallId>("signaling:force-call-end", event => {
                removeCall(event.payload);
            }),
            listen<{callId: CallId; targets: CallTarget[]}>("signaling:call-reject", event => {
                rejectTargets(event.payload.callId, event.payload.targets);
            }),
            listen<IncomingCallListEntry>("signaling:add-incoming-to-call-list", event => {
                addIncomingCallListEntry(event.payload);
            }),
            listen<Profile>("signaling:test-profile", event => {
                closeErrorOverlayIfTitle("Profile error");
                setConnectionState("test");
                resetProfileStore(false);
                setProfile(event.payload);
                goToPage("phone");
            }),
            listen<ClientPageSettings>("signaling:client-page-config", event => {
                setClientPageSettings(event.payload);
            }),
        );
    };

    init();

    return () => {
        unlistenFns.forEach(fn => fn.then(f => f()));
    };
}
