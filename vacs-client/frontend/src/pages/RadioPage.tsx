import FrequencyObject from "../components/radio/FrequencyObject.tsx";
import {useEffect, useState} from "preact/hooks";
import {invokeSafe} from "../error.ts";
import {RadioState, RadioStation} from "../types/radio.ts";
import {listen, UnlistenFn} from "../transport";
import AddRadioStation from "../components/radio/AddRadioStation.tsx";
import {sortCallsigns} from "../types/client.ts";
import {useRadioStore} from "../stores/radio-store.ts";
import {setPage} from "../stores/navigation-store.ts";
import {useSettingsStore} from "../stores/settings-store.ts";

function RadioPage() {
    const radioState = useRadioStore(state => state.radioState);

    const radioIsTrackAudio = useSettingsStore(
        state => state.radioConfig?.integration === "TrackAudio",
    );

    useEffect(() => {
        if (
            !radioIsTrackAudio ||
            radioState?.state === undefined ||
            radioState?.state === "NotConfigured"
        ) {
            setPage("phone"); // TODO: this might change (if radio is not trackaudio but view is split, then this might do something unintended; what should it do? fullscreen phone page prbly)
        }
    }, [radioState?.state]);

    const radioConnected =
        radioState?.state !== "NotConfigured" && radioState?.state !== "Disconnected";

    return radioIsTrackAudio ? (
        radioConnected ? (
            <RadioPageInner radioState={radioState} />
        ) : (
            <div className="w-full h-full p-1 flex flex-col justify-center items-center text-slate-600">
                <p>No TrackAudio radio connection.</p>
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
        const fetch = async () => {
            const stations = await invokeSafe<RadioStation[]>("radio_get_stations");
            if (stations === undefined) return;
            setStations(new Map(stations.map(station => [station.frequency, station])));
        };
        void fetch();

        const unlistenFns: Promise<UnlistenFn>[] = [];

        unlistenFns.push(
            listen<RadioStation>("radio:station-added", event => {
                setStations(prev => prev.set(event.payload.frequency, event.payload));
            }),
            listen<number>("radio:station-removed", event => {
                setStations(prev => {
                    prev.delete(event.payload);
                    return prev;
                });
            }),
            listen<RadioStation>("radio:station-updated", event => {
                setStations(prev => prev.set(event.payload.frequency, event.payload));
            }),
            listen<RadioStation[]>("radio:stations-synced", event =>
                setStations(new Map(event.payload.map(station => [station.frequency, station]))),
            ),
        );

        return () => {
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

const PRIORITY = ["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND", "*_DEL"];
function sortRadioStations(a: [number, RadioStation], b: [number, RadioStation]): number {
    const aCallsign = a[1].callsign ?? "";
    const bCallsign = b[1].callsign ?? "";
    return sortCallsigns(aCallsign, bCallsign, PRIORITY, true);
}

export default RadioPage;
