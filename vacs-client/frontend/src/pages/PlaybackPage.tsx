import {clsx} from "clsx";
import {useEffect, useLayoutEffect, useRef, useState} from "preact/hooks";
import PlaybackActions from "../components/playback/PlaybackActions.tsx";
import {PlaybackControls} from "../components/playback/PlaybackControls.tsx";
import PlaybackList from "../components/playback/PlaybackList.tsx";
import PlaybackProgress from "../components/playback/PlaybackProgress.tsx";
import Button from "../components/ui/Button.tsx";
import {invokeSafe} from "../error.ts";
import {usePlaybackControls} from "../hooks/playback-controls-hook.ts";
import {useCapabilitiesStore} from "../stores/capabilities-store.ts";
import {openSettingsSubmenu} from "../stores/navigation-store.ts";
import {isPlaybackRoot, usePlaybackStore} from "../stores/playback-store.ts";
import {useRadioStore} from "../stores/radio-store.ts";
import {useSettingsStore} from "../stores/settings-store.ts";
import {listen, UnlistenFn} from "../transport";
import {INSTANCE_ID} from "../transport/store-sync.ts";
import {ClipMeta, sortClips} from "../types/playback.ts";
import {CloseButton} from "./SettingsPage.tsx";

function PlaybackPage() {
    const capPlayback = useCapabilitiesStore(state => state.playback);
    const capPlatform = useCapabilitiesStore(state => state.platform);

    const radioEnabled = useSettingsStore(state => state.radioConfig?.integration != null);

    const playbackEnabled = useSettingsStore(state => state.playbackEnabled);

    const radioConnected = useRadioStore(
        state =>
            state.radioState?.state !== "NotConfigured" &&
            state.radioState?.state !== "Disconnected",
    );

    return (
        <div
            className={clsx(
                "z-10 absolute h-[calc(100%+3px)] w-[44rem] -top-px right-[-2px]",
                "bg-blue-700 px-2 pb-2 flex flex-col rounded-md",
            )}
        >
            <p className="w-full text-white bg-blue-700 font-semibold text-center">Playback</p>
            {!capPlayback ? (
                <div className="w-full grow rounded-b-sm bg-[#B5BBC6] flex justify-center items-center text-slate-600">
                    Radio playback is not yet supported on {capPlatform}.
                </div>
            ) : !radioEnabled ? (
                <div className="w-full grow rounded-b-sm bg-[#B5BBC6] flex flex-col justify-center items-center text-slate-600">
                    <p>TrackAudio radio integration is not enabled.</p>
                    <p>
                        Enable it in the{" "}
                        <span
                            className="text-blue-700 cursor-pointer"
                            onClick={() => {
                                void invokeSafe("audio_play_ui_click");
                                openSettingsSubmenu("settings-transmit");
                            }}
                        >
                            transmit settings
                        </span>
                        .
                    </p>
                </div>
            ) : !playbackEnabled ? (
                <div className="w-full grow rounded-b-sm bg-[#B5BBC6] flex flex-col justify-center items-center text-slate-600">
                    <p>Radio playback is not enabled.</p>
                    <p>
                        Enable it in the{" "}
                        <span
                            className="text-blue-700 cursor-pointer"
                            onClick={() => {
                                void invokeSafe("audio_play_ui_click");
                                openSettingsSubmenu("settings-advanced");
                            }}
                        >
                            advanced settings
                        </span>
                        .
                    </p>
                </div>
            ) : !radioConnected ? (
                <div className="w-full grow rounded-b-sm bg-[#B5BBC6] flex flex-col justify-center items-center text-slate-600">
                    <p>No radio connection.</p>
                </div>
            ) : (
                <PlaybackPageInner />
            )}
        </div>
    );
}

function PlaybackPageInner() {
    const [clips, setClips] = useState<ClipMeta[]>([]);
    const clipsRef = useRef<ClipMeta[]>([]);

    const selected = usePlaybackStore(state => state.selected);
    const {setSelected} = usePlaybackStore(state => state.actions);

    const selectedClip = clips[selected];
    const prevClip = clips[selected + 1];
    const nextClip = clips[selected - 1];

    const controls = usePlaybackControls({clips, selectedClip, prevClip, nextClip});
    const {active, handleStop} = controls;

    useLayoutEffect(() => {
        clipsRef.current = clips;
    }, [clips]);

    useEffect(() => {
        usePlaybackStore.getState().actions.setOpenInstanceIds(prev => [...prev, INSTANCE_ID]);

        const fetch = async () => {
            const list = await invokeSafe<ClipMeta[]>("playback_list");
            if (list === undefined) return;
            setClips(sortClips(list));
        };
        void fetch();

        const unlistenFns: Promise<UnlistenFn>[] = [];
        unlistenFns.push(
            listen<{recorded: ClipMeta; evicted: ClipMeta[]}>("playback:clips-modified", event => {
                const status = usePlaybackStore.getState().status;
                const evictedIds = new Set(event.payload.evicted.map(c => c.id));
                const filtered = clipsRef.current.filter(c => !evictedIds.has(c.id));
                const playingEvicted = status !== undefined && evictedIds.has(status.id);

                if (playingEvicted && isPlaybackRoot()) void handleStop();
                if (filtered.length > 0 && status !== undefined && !playingEvicted) {
                    setSelected(prev => prev + 1);
                }
                setClips(sortClips([...filtered, event.payload.recorded]));
            }),
        );

        return () => {
            const ownsPlayback = usePlaybackStore.getState().status?.sayAgain !== true;
            usePlaybackStore.getState().actions.setOpenInstanceIds(prev => {
                const next = prev.filter(id => id !== INSTANCE_ID);
                if (next.length === 0 && ownsPlayback) void handleStop();
                return next;
            });
            unlistenFns.forEach(fn => fn.then(f => f()));
        };
    }, [handleStop, setSelected]);

    return (
        <div className="w-full grow rounded-b-sm bg-[#B5BBC6] grid grid-cols-[6.5rem_auto] p-2 gap-2 overflow-auto">
            <div className="h-full w-full flex flex-col justify-between items-center">
                <div className="w-full flex flex-col items-center bg-gray-300 border rounded-md">
                    <p className="w-full border-b text-center font-semibold">Filter</p>
                    <Button color="gray" className="h-15 my-2 uppercase">
                        <p>
                            Speech <br /> Only
                        </p>
                    </Button>
                    <Button color="blue" className="h-15 mt-2 uppercase rounded-b-none!">
                        Radio
                    </Button>
                    <Button color="gray" className="h-15 mb-2 uppercase rounded-t-none!">
                        Phone
                    </Button>
                </div>
                <PlaybackActions
                    clips={clips}
                    selectedClip={selectedClip}
                    setClips={setClips}
                    deleteDisabled={active}
                />
            </div>
            <div className="h-full w-full flex flex-col p-px">
                <PlaybackList clips={clips} selected={selected} setSelected={setSelected} />
                <div className="relative w-full h-full flex flex-col items-center pr-16">
                    <PlaybackProgress clip={active ? clips[selected] : undefined} />
                    <div className="flex-1 min-h-0 w-full flex items-end justify-center mt-[0.625rem]">
                        <PlaybackControls
                            selectedClip={selectedClip}
                            prevClip={prevClip}
                            nextClip={nextClip}
                            status={controls.status}
                            playbackDevice={controls.playbackDevice}
                            active={active}
                            onPlayPause={controls.handlePlayPause}
                            onStop={controls.handleStop}
                            onSeekBack={controls.handleSeekBack}
                            onSeekForward={controls.handleSeekForward}
                            onDeviceSwitch={controls.handleDeviceSwitch}
                            onPrev={controls.handlePrev}
                            onNext={controls.handleNext}
                        />
                    </div>
                    <CloseButton className="h-17 w-19! absolute bottom-0 right-0" />
                </div>
            </div>
        </div>
    );
}

export default PlaybackPage;
