import type {ClientSessionInfo} from "../types/client.ts";
import {useConnectionStore} from "./connection-store.ts";
import {useProfileStore} from "./profile-store.ts";
import {useStationsStore} from "./stations-store.ts";
import {useCallStore} from "./call-store.ts";

export function applySessionInfo(info: ClientSessionInfo) {
    useConnectionStore.getState().setConnectionInfo(info.client);
    if (info.profile.type === "changed" && info.profile.activeProfile?.profile) {
        useProfileStore
            .getState()
            .setProfile(info.profile.activeProfile.profile, info.splitProfileWidth);
    }
    useStationsStore.getState().setPositionDefaultSources(info.defaultCallSources);
    useCallStore.getState().actions.setMaxConferenceSize(info.maxConfSize);
}
