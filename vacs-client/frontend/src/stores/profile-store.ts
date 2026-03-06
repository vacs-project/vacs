import {
    DirectAccessKey,
    DirectAccessPage,
    GeoPageContainer,
    isGeoPageButton,
    isGeoPageContainer,
    Profile,
} from "../types/profile.ts";
import {create} from "zustand/react";
import {useShallow} from "zustand/react/shallow";

type SelectedPage = {current: DirectAccessPage | undefined; parent: DirectAccessPage | undefined};

type ProfileState = {
    profile: Profile | undefined;
    page: SelectedPage;
    testProfilePath: string | undefined;
    setProfile: (profile: Profile | undefined) => void;
    setPage: (page: DirectAccessPage | undefined) => void;
    setSubpage: (page: DirectAccessPage, parent: DirectAccessPage) => void;
    navigateParentPage: () => void;
    setTestProfilePath: (path: string | undefined) => void;
    reset: (resetTestProfilePath?: boolean) => void;
};

export const useProfileStore = create<ProfileState>()((set, get, store) => ({
    profile: undefined,
    page: {current: undefined, parent: undefined},
    testProfilePath: undefined,
    setProfile: profile => set({profile}),
    setPage: page => set({page: {current: page, parent: undefined}}),
    setSubpage: (page, parent) => set({page: {current: page, parent: get().page.parent ?? parent}}),
    navigateParentPage: () => {
        const parent = get().page.parent;
        if (parent === undefined) return;
        set({page: {current: parent, parent: undefined}});
    },
    setTestProfilePath: path => set({testProfilePath: path}),
    reset: (resetTestProfilePath = true) =>
        set({
            ...store.getInitialState(),
            testProfilePath: resetTestProfilePath ? undefined : get().testProfilePath,
        }),
}));

export const useProfileType = (): "geo" | "tabbed" | "unknown" | undefined => {
    return useProfileStore(state => {
        if (state.profile === undefined) return undefined;
        if (state.profile.geo !== undefined) return "geo";
        if (state.profile.tabbed !== undefined) return "tabbed";
        return "unknown";
    });
};

const profileToStationKeys = (profile: Profile | undefined): DirectAccessKey[] => {
    if (profile?.tabbed !== undefined) {
        return profile.tabbed.flatMap(t =>
            directAccessPageToStationKeys(t.page).filter(k => k.stationId !== undefined),
        );
    }
    if (profile?.geo !== undefined) {
        return geoPageContainerToKeys(profile.geo).filter(k => k.stationId !== undefined);
    }
    return [];
};

export const getProfileStationKeysState = () => {
    return profileToStationKeys(useProfileStore.getState().profile);
};

export const useProfileStationKeys = () => {
    return useProfileStore(useShallow(state => profileToStationKeys(state.profile)));
};

export function directAccessPageToStationKeys(
    page: DirectAccessPage | undefined,
): DirectAccessKey[] {
    const result: DirectAccessKey[] = [];

    function visit(page: DirectAccessPage | undefined) {
        if (page === undefined || page.keys === undefined) return;

        for (const key of page.keys) {
            if (key.stationId !== undefined) result.push(key);
            visit(key.page);
        }
    }

    visit(page);

    return result;
}

function geoPageContainerToKeys(container: GeoPageContainer): DirectAccessKey[] {
    return container.children.flatMap(c => {
        if (isGeoPageContainer(c)) {
            return geoPageContainerToKeys(c);
        } else if (isGeoPageButton(c)) {
            return directAccessPageToStationKeys(c.page);
        }
        return [];
    });
}
