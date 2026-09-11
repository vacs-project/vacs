import {TargetedEvent} from "preact";
import {invokeStrict} from "../../error.ts";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {usePlaybackStore} from "../../stores/playback-store.ts";
import {useSettingsStore} from "../../stores/settings-store.ts";
import Checkbox from "../ui/Checkbox.tsx";

function PlaybackSettings() {
    const enabled = useSettingsStore(state => state.playbackEnabled);
    const setEnabled = useSettingsStore(state => state.setPlaybackEnabled);

    const handleToggle = useAsyncDebounce(async (e: TargetedEvent<HTMLInputElement>) => {
        const next = e.currentTarget.checked;
        setEnabled(next);
        try {
            await invokeStrict("playback_set_enabled", {enabled: next});
            if (!next) usePlaybackStore.getState().actions.setStatus(undefined);
        } catch {
            setEnabled(!next);
        }
    });

    return (
        <div className="w-full flex justify-between items-center">
            <label htmlFor="playback-enabled">Enable radio playback</label>
            <Checkbox name="playback-enabled" checked={enabled} onChange={handleToggle} />
        </div>
    );
}

export default PlaybackSettings;
