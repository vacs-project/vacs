import {useEffect} from "preact/hooks";
import {setPage, useNavigationStore} from "../stores/navigation-store.ts";
import {useProfileStore, useProfileType} from "../stores/profile-store.ts";
import {useSettingsStore} from "../stores/settings-store.ts";

export function useSplitView() {
    const profileType = useProfileType();
    const profileView = useProfileStore(state => state.profile?.view);
    const radioIsTrackAudio = useSettingsStore(
        state => state.radioConfig?.integration === "TrackAudio",
    );

    return profileType === "tabbed" && profileView !== undefined && profileView !== "page"
        ? radioIsTrackAudio
        : false;
}

export function usePageSync() {
    const page = useNavigationStore(state => state.page);
    const profileId = useProfileStore(state => state.profile?.id);
    const radioConfigLoaded = useSettingsStore(state => state.radioConfig !== undefined);
    const radioIsTrackAudio = useSettingsStore(
        state => state.radioConfig?.integration === "TrackAudio",
    );
    const splitView = useSplitView();

    useEffect(() => {
        if (profileId === undefined || !radioConfigLoaded || !splitView) return;
        setPage("split");
    }, [profileId, radioConfigLoaded, splitView]);

    useEffect(() => {
        if ((page === "split" && !splitView) || (page === "radio" && !radioIsTrackAudio)) {
            setPage("phone");
        }
    }, [page, splitView, radioIsTrackAudio]);
}
