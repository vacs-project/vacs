import FrequencyObject from "../components/radio/FrequencyObject.tsx";
import {useEffect, useState} from "preact/hooks";
import {invokeSafe} from "../error.ts";
import {RadioState, RadioStation} from "../types/radio.ts";
import {listen, UnlistenFn} from "../transport";
import AddRadioStation from "../components/radio/AddRadioStation.tsx";
import {sortCallsigns} from "../types/client.ts";
import {useRadioStore} from "../stores/radio-store.ts";
import {useSettingsStore} from "../stores/settings-store.ts";

function RadioPage() {
    const radioState = useRadioStore(state => state.radioState);

    const radioIsTrackAudio = useSettingsStore(
        state => state.radioConfig?.integration === "TrackAudio",
    );

    const radioConnected =
        radioState?.state !== "NotConfigured" &&
        radioState?.state !== "Disconnected" &&
        radioState?.state !== "Error";

    return radioIsTrackAudio ? (
        radioState?.state === "Connected" ? (
            <div className="w-full h-full p-1 flex flex-col justify-center items-center text-slate-600 text-center">
                <p>TrackAudio is not connected to VATSIM voice.</p>
            </div>
        ) : radioConnected ? (
            <RadioPageInner radioState={radioState} />
        ) : (
            <div className="w-full h-full p-1 flex flex-col justify-center items-center text-slate-600 text-center">
                <p>
                    {radioState?.state === "Error"
                        ? "TrackAudio radio connection failed."
                        : "No TrackAudio radio connection."}
                </p>
                <p
                    className="text-blue-700 cursor-pointer"
                    onClick={() => {
                        void invokeSafe("audio_play_ui_click");
                        void invokeSafe("radio_reconnect");
                    }}
                >
                    Retry
                </p>
            </div>
        )
    ) : (
        <></>
    );
}

function RadioPageInner({radioState}: {radioState: RadioState | undefined}) {
    const [stations, setStations] = useState<Map<number, RadioStation>>(new Map());

    useEffect(() => {
        let active = true;
        // Events that arrive while the snapshot is in flight are replayed on top of it, otherwise
        // the snapshot would drop stations TrackAudio announced in the meantime.
        let pending: StationsUpdate[] | undefined = [];

        const apply = (update: StationsUpdate) => {
            pending?.push(update);
            setStations(update);
        };

        const upsert = (station: RadioStation) =>
            apply(prev => new Map(prev).set(station.frequency, station));

        const unlistenFns: Promise<UnlistenFn>[] = [
            listen<RadioStation>("radio:station-added", event => upsert(event.payload)),
            listen<number>("radio:station-removed", event =>
                apply(prev => {
                    const next = new Map(prev);
                    next.delete(event.payload);
                    return next;
                }),
            ),
            listen<RadioStation>("radio:station-updated", event => upsert(event.payload)),
            listen<RadioStation[]>("radio:stations-synced", event =>
                apply(() => toStationMap(event.payload)),
            ),
        ];

        void Promise.all(unlistenFns).then(async () => {
            const snapshot = await invokeSafe<RadioStation[]>("radio_get_stations");
            if (!active) return;

            const replay = pending ?? [];
            pending = undefined;
            if (snapshot === undefined) return;

            setStations(
                replay.reduce((stations, update) => update(stations), toStationMap(snapshot)),
            );
        });

        return () => {
            active = false;
            unlistenFns.forEach(fn => fn.then(f => f()));
        };
    }, []);

    return (
        <div className="w-full h-full p-1.5 pr-0 flex flex-wrap-reverse content-start gap-2 overflow-y-auto">
            {Array.from(stations.entries())
                .sort(sortRadioStations)
                .map(([freq, station]) => (
                    <FrequencyObject
                        key={freq}
                        station={station}
                        rxActive={
                            radioState?.state === "RxActive" &&
                            (radioState?.data?.includes(freq) ?? false)
                        }
                        txActive={radioState?.state === "TxActive"}
                    />
                ))}
            <AddRadioStation />
        </div>
    );
}

type StationsUpdate = (stations: Map<number, RadioStation>) => Map<number, RadioStation>;

const toStationMap = (stations: RadioStation[]) =>
    new Map(stations.map(station => [station.frequency, station]));

const PRIORITY = ["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND", "*_DEL"];
function sortRadioStations(a: [number, RadioStation], b: [number, RadioStation]): number {
    const aCallsign = a[1].callsign ?? "";
    const bCallsign = b[1].callsign ?? "";
    return sortCallsigns(aCallsign, bCallsign, PRIORITY, true);
}

export default RadioPage;
