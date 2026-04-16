import "../../styles/frequency-object.css";
import Button from "../ui/Button.tsx";
import {clsx} from "clsx";
import {RadioStation, StationStateUpdate} from "../../types/radio.ts";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {invokeSafe} from "../../error.ts";
import {useRadioStore} from "../../stores/radio-store.ts";
import xc from "../../assets/xc.svg";
import speaker from "../../assets/speaker.svg";

type FrequencyObjectProps = {
    station: RadioStation;
    rxActive: boolean;
    txActive: boolean;
};

function FrequencyObject({station, rxActive, txActive}: FrequencyObjectProps) {
    const update = useAsyncDebounce(async (update: StationStateUpdate) => {
        await invokeSafe("radio_set_station_state", {update, frequency: station.frequency});
    });

    return (
        <div className="grid grid-rows-2 grid-cols-[55%_45%] h-[6.188rem] w-42 bg-gray-300 rounded-md border-gray-800 override-gray">
            <div
                className="border border-gray-800 rounded-tl-md rounded-bl-md"
                style={{gridRow: `span 2 / span 2`}}
            >
                <Button
                    color="gray"
                    className="h-full w-full outline-0! rounded-tr-none rounded-br-none rounded-tl-md rounded-bl-md flex flex-col justify-center items-center font-semibold"
                    onClick={() => {
                        if (useRadioStore.getState().cpl) {
                            void update({xca: !station.xca});
                        }
                    }}
                >
                    <div className="flex-1 min-h-0 flex flex-col justify-center">
                        <p
                            className={clsx(
                                "leading-5",
                                callsignTextSize(station.callsign?.length ?? 0),
                            )}
                        >
                            {station.callsign}
                        </p>
                        <p className="text-lg leading-5">{formatFrequency(station.frequency)}</p>
                    </div>
                    <div className="w-full h-px bg-gray-700" />
                    <div className="w-full flex-1 min-h-0">
                        <div className="h-full flex justify-between items-center pl-2.5 pr-3">
                            <img
                                src={speaker}
                                alt="XC"
                                className={clsx("w-6 h-6", station.headset && "opacity-50")}
                                onClick={() => update({headset: !station.headset})}
                            />
                            <p className={clsx(!station.xca && "hidden")}>
                                <img src={xc} alt="XC" className="w-5.5 h-5.5" />
                            </p>
                        </div>
                    </div>
                </Button>
            </div>
            <div className="border border-l-0 border-gray-800 rounded-tr-md">
                <Button
                    color={
                        station.rx
                            ? rxActive || (station.tx && txActive)
                                ? "cornflower"
                                : "emerald"
                            : "gray"
                    }
                    className="h-full w-full outline-0! rounded-none! rounded-tr-md! flex justify-center items-center font-semibold text-lg"
                    onClick={() => update({rx: !station.rx})}
                >
                    Rx
                </Button>
            </div>
            <div className="border border-l-0 border-t-0 border-gray-800 rounded-br-md">
                <Button
                    color={station.tx ? (txActive ? "cornflower" : "emerald") : "gray"}
                    className="h-full w-full outline-0! rounded-none! rounded-br-md! flex justify-center items-center font-semibold text-lg"
                    onClick={() => update({tx: !station.tx})}
                >
                    Tx
                </Button>
            </div>
        </div>
    );
}

function callsignTextSize(length: number): string {
    if (length <= 7) return "text-lg";
    if (length == 8) return "";
    if (length == 9) return "text-sm";
    return "text-xs";
}

function formatFrequency(freq: number): string {
    return (freq / 1_000_000).toFixed(3);
}

export default FrequencyObject;
