import FrequencyObject from "../components/radio/FrequencyObject.tsx";
import {useEffect, useState} from "preact/hooks";
import {invokeSafe} from "../error.ts";
import {RadioState, RadioStation} from "../types/radio.ts";
import {listen, UnlistenFn} from "../transport";
import AddRadioStation from "../components/radio/AddRadioStation.tsx";
import {sortCallsigns} from "../types/client.ts";

function RadioPage() {
    const [stations, setStations] = useState<Map<number, RadioStation>>(new Map());
    const [radioState, setRadioState] = useState<RadioState | undefined>(undefined);

    useEffect(() => {
        if (
            radioState?.state !== undefined &&
            (radioState.state === "NotConfigured" || radioState.state === "Disconnected")
        ) {
            window.history.back();
        }
    }, [radioState?.state]);

    useEffect(() => {
        const fetch = async () => {
            const stations = await invokeSafe<RadioStation[]>("radio_get_stations");
            if (stations === undefined) return;
            setStations(new Map(stations.map(station => [station.frequency, station])));

            const state = await invokeSafe<RadioState>("keybinds_get_radio_state");
            if (state === undefined) return;
            setRadioState(state);
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
            listen<RadioState>("radio:state", event => setRadioState(event.payload)),
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

const PRIORITY = ["*_DEL", "*_GND", "*_TWR", "*_APP", "*_CTR", "*_FMP"];
function sortRadioStations(a: [number, RadioStation], b: [number, RadioStation]): number {
    const aCallsign = a[1].callsign ?? "";
    const bCallsign = b[1].callsign ?? "";
    return sortCallsigns(aCallsign, bCallsign, PRIORITY, true);
}

export default RadioPage;
