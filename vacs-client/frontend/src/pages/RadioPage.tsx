import FrequencyObject from "../components/radio/FrequencyObject.tsx";
import {useEffect, useState} from "preact/hooks";
import {invokeSafe} from "../error.ts";
import {RadioState, RadioStation} from "../types/radio.ts";
import {listen, UnlistenFn} from "../transport";

function RadioPage() {
    const [stations, setStations] = useState<RadioStation[]>([]);
    const [radioState, setRadioState] = useState<RadioState>({state: "NotConfigured"});

    useEffect(() => {
        const fetch = async () => {
            const stations = await invokeSafe<RadioStation[]>("radio_get_stations");
            if (stations === undefined) return;
            setStations(stations);

            const state = await invokeSafe<RadioState>("keybinds_get_radio_state");
            if (state === undefined) return;
            setRadioState(state);
        };
        void fetch();

        const unlistenFns: Promise<UnlistenFn>[] = [];

        unlistenFns.push(
            listen<RadioStation>("radio:station-added", event => {
                setStations(prev => [...prev, event.payload]);
            }),
            listen<number>("radio:station-removed", event => {
                setStations(prev => prev.filter(station => station.frequency !== event.payload));
            }),
            listen<RadioStation>("radio:station-updated", event => {
                setStations(prev =>
                    prev.map(station =>
                        station.frequency === event.payload.frequency ? event.payload : station,
                    ),
                );
            }),
            listen<RadioStation[]>("radio:stations-synced", event => setStations(event.payload)),
            listen<RadioState>("radio:state", event => setRadioState(event.payload)),
        );

        return () => {
            unlistenFns.forEach(fn => fn.then(f => f()));
        };
    }, []);

    return (
        <div className="w-full h-full p-1.5 pr-0 flex flex-wrap-reverse content-start gap-2 overflow-y-auto">
            {stations.map(station => (
                <FrequencyObject
                    key={station.frequency}
                    station={station}
                    rxActive={
                        radioState.state === "RxActive" &&
                        (radioState.data?.includes(station.frequency) ?? false)
                    }
                    txActive={radioState.state === "TxActive"}
                />
            ))}
        </div>
    );
}

export default RadioPage;
