import {TargetedEvent} from "preact";
import {invokeStrict} from "../../error.ts";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {usePlaybackStore} from "../../stores/playback-store.ts";
import {useSettingsStore} from "../../stores/settings-store.ts";
import Checkbox from "../ui/Checkbox.tsx";

function PlaybackSettings() {
    const enabled = useSettingsStore(state => state.playbackEnabled);
    const setEnabled = useSettingsStore(state => state.setPlaybackEnabled);
    const sayAgainEnabled = useSettingsStore(state => state.sayAgainEnabled);
    const setSayAgainEnabled = useSettingsStore(state => state.setSayAgainEnabled);

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

    const handleSayAgainToggle = useAsyncDebounce(async (e: TargetedEvent<HTMLInputElement>) => {
        const next = e.currentTarget.checked;
        setSayAgainEnabled(next);
        try {
            await invokeStrict("playback_set_say_again", {enabled: next});
        } catch {
            setSayAgainEnabled(!next);
            return;
        }
        if (next || usePlaybackStore.getState().status?.sayAgain !== true) return;
        try {
            await invokeStrict("playback_stop");
            usePlaybackStore.getState().actions.setStatus(undefined);
        } catch {}
    });

    return (
        <>
            <div className="w-full flex justify-between items-center">
                <label htmlFor="playback-enabled">Enable radio playback</label>
                <Checkbox name="playback-enabled" checked={enabled} onChange={handleToggle} />
            </div>
            <div className="w-full flex justify-between items-center">
                <label htmlFor="say-again-enabled">Show SAY AGAIN button</label>
                <Checkbox
                    name="say-again-enabled"
                    checked={sayAgainEnabled}
                    onChange={handleSayAgainToggle}
                />
            </div>
        </>
    );
}

export default PlaybackSettings;
