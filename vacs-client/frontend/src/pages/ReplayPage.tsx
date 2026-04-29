import {useEffect, useRef, useState} from "preact/hooks";
import {invokeSafe, invokeStrict} from "../error.ts";
import {listen, UnlistenFn} from "../transport";
import Button from "../components/ui/Button.tsx";
import {ClipMeta, clipUnixMs, tapLabel} from "../types/replay.ts";

function ReplayPage() {
    const [clips, setClips] = useState<ClipMeta[]>([]);
    const [activeId, setActiveId] = useState<number | null>(null);
    const audioRef = useRef<HTMLAudioElement | null>(null);
    const objectUrlRef = useRef<string | null>(null);

    useEffect(() => {
        const fetch = async () => {
            const list = await invokeSafe<ClipMeta[]>("replay_list");
            if (list === undefined) return;
            setClips(sortClips(list));
        };
        void fetch();

        const unlistenFns: Promise<UnlistenFn>[] = [];
        unlistenFns.push(
            listen<ClipMeta>("replay:clip-recorded", event => {
                setClips(prev => sortClips([...prev, event.payload]));
            }),
            listen<ClipMeta>("replay:clip-evicted", event => {
                setClips(prev => prev.filter(c => c.id !== event.payload.id));
            }),
        );

        return () => {
            unlistenFns.forEach(fn => fn.then(f => f()));
            if (objectUrlRef.current !== null) {
                URL.revokeObjectURL(objectUrlRef.current);
                objectUrlRef.current = null;
            }
        };
    }, []);

    const handlePlay = async (clip: ClipMeta) => {
        try {
            const bytes = await invokeStrict<number[]>("replay_get_clip_bytes", {id: clip.id});
            const blob = new Blob([new Uint8Array(bytes)], {type: "audio/wav"});
            if (objectUrlRef.current !== null) {
                URL.revokeObjectURL(objectUrlRef.current);
            }
            objectUrlRef.current = URL.createObjectURL(blob);
            setActiveId(clip.id);
            if (audioRef.current !== null) {
                audioRef.current.src = objectUrlRef.current;
                void audioRef.current.play();
            }
        } catch {
            // Error overlay surfaces from invokeStrict; nothing to do here.
        }
    };

    const handleDelete = async (clip: ClipMeta) => {
        await invokeSafe("replay_delete", {id: clip.id});
        setClips(prev => prev.filter(c => c.id !== clip.id));
        if (activeId === clip.id) setActiveId(null);
    };

    const handleExport = async (clip: ClipMeta) => {
        await invokeSafe("replay_export", {id: clip.id});
    };

    const handleClear = async () => {
        await invokeSafe("replay_clear");
        setClips([]);
        setActiveId(null);
    };

    return (
        <div className="w-full h-full p-2 flex flex-col gap-2 overflow-hidden">
            <div className="flex flex-row justify-between items-center px-1">
                <p className="font-semibold">Replay buffer</p>
                <Button color="cyan" onClick={handleClear} disabled={clips.length === 0}>
                    Clear all
                </Button>
            </div>
            <div className="flex-1 min-h-0 overflow-y-auto flex flex-col gap-1 pr-1">
                {clips.length === 0 ? (
                    <p className="text-center text-slate-600 mt-4">No clips recorded yet.</p>
                ) : (
                    clips.map(clip => (
                        <ClipRow
                            key={clip.id}
                            clip={clip}
                            active={activeId === clip.id}
                            onPlay={() => void handlePlay(clip)}
                            onDelete={() => void handleDelete(clip)}
                            onExport={() => void handleExport(clip)}
                        />
                    ))
                )}
            </div>
            <audio ref={audioRef} controls className="w-full" />
        </div>
    );
}

type ClipRowProps = {
    clip: ClipMeta;
    active: boolean;
    onPlay: () => void;
    onDelete: () => void;
    onExport: () => void;
};

function ClipRow({clip, active, onPlay, onDelete, onExport}: ClipRowProps) {
    const time = new Date(clipUnixMs(clip.started_at)).toLocaleTimeString();
    const duration = (clip.duration_ms / 1000).toFixed(1);
    const label = clip.callsign ?? tapLabel(clip.tap);
    return (
        <div
            className={`flex flex-row items-center gap-2 px-2 py-1 rounded border ${
                active ? "bg-blue-100 border-blue-400" : "bg-gray-200 border-gray-400"
            }`}
        >
            <div className="flex-1 min-w-0 flex flex-col">
                <span className="font-semibold truncate">{label}</span>
                <span className="text-xs text-slate-600">
                    {time} &middot; {duration}s &middot; {tapLabel(clip.tap)}
                </span>
            </div>
            <Button color="cyan" onClick={onPlay}>
                Play
            </Button>
            <Button color="cyan" onClick={onExport}>
                Save
            </Button>
            <Button color="salmon" onClick={onDelete}>
                Delete
            </Button>
        </div>
    );
}

function sortClips(list: ClipMeta[]): ClipMeta[] {
    return [...list].sort((a, b) => clipUnixMs(b.started_at) - clipUnixMs(a.started_at));
}

export default ReplayPage;
