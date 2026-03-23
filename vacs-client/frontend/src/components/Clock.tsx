import {useEffect, useRef, useState} from "preact/hooks";
import ConnectionStatusIndicator from "./ui/ConnectionStatusIndicator.tsx";
import {useSettingsStore} from "../stores/settings-store.ts";
import {invokeStrict} from "../error.ts";
import {useAsyncDebounce} from "../hooks/debounce-hook.ts";
import {ClockMode} from "../types/settings.ts";

type TimeState = {
    hours: string;
    minutes: string;
    seconds: string;
    day: string;
};

function Clock() {
    const mode = useSettingsStore(state => state.clockMode);
    const setMode = useSettingsStore(state => state.setClockMode);
    const intervalRef = useRef<number | undefined>(undefined);
    const [time, setTime] = useState<TimeState>({
        hours: "99",
        minutes: "99",
        seconds: "99",
        day: "99",
    });

    useEffect(() => {
        const updateClock = () => {
            const now = new Date();
            const hours = now.getUTCHours();
            const minutes = now.getUTCMinutes();
            const seconds = now.getUTCSeconds();
            const day = now.getUTCDate();

            setTime({
                hours: toClockString(hours),
                minutes: toClockString(minutes),
                seconds: toClockString(mode === "Relaxed" ? seconds - (seconds % 10) : seconds),
                day: toClockString(day),
            });

            switch (mode) {
                case "Realtime":
                    return 1000 - now.getUTCMilliseconds();
                case "Relaxed":
                    return 10 * 1000 - (now.getUTCMilliseconds() + (seconds % 10) * 1000);
                case "Day":
                    return 60 * 1000 - (now.getUTCMilliseconds() + now.getUTCSeconds() * 1000);
            }
        };

        const interval = mode === "Realtime" ? 1000 : mode === "Relaxed" ? 10 * 1000 : 60 * 1000;

        const diff = updateClock();
        const timeout = setTimeout(() => {
            updateClock();
            intervalRef.current = setInterval(updateClock, interval);
        }, diff);

        return () => {
            clearTimeout(timeout);
            clearInterval(intervalRef.current);
        };
    }, [mode]);

    const handleOnClick = useAsyncDebounce(async () => {
        const nextMode: ClockMode =
            mode === "Realtime" ? "Relaxed" : mode === "Relaxed" ? "Day" : "Realtime";
        try {
            await invokeStrict("app_set_clock_mode", {clockMode: nextMode});
            setMode(nextMode);
        } catch {}
    });

    const title =
        mode === "Realtime"
            ? "Updates every second. Click to switch to Relaxed."
            : mode === "Relaxed"
              ? "Updates every 10 seconds. Click to switch to Day."
              : "Displays the current day instead of seconds. Click to switch to Realtime.";

    return (
        <div
            className="h-full px-1 border-r bg-[#c3c8ce] w-min whitespace-nowrap cursor-pointer"
            onClick={handleOnClick}
            title={title}
        >
            <div className="h-1/2 flex items-center">
                <p className="font-bold leading-3 tracking-wider text-xl">
                    {time.hours}:{time.minutes}
                </p>
            </div>
            <div className="h-1/2 flex items-center justify-between">
                <div className="h-full py-1.5 pl-0.5">
                    <ConnectionStatusIndicator />
                </div>
                <p className="font-bold leading-3 tracking-wider text-xl text-gray-500">
                    {mode === "Day" ? time.day : time.seconds}
                </p>
            </div>
        </div>
    );
}

const toClockString = (time: number) => time.toString().padStart(2, "0");

export default Clock;
