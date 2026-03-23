import {useEffect, useRef, useState} from "preact/hooks";
import ConnectionStatusIndicator from "./ui/ConnectionStatusIndicator.tsx";

type TimeState = {
    hours: string;
    minutes: string;
    seconds: string;
};

function Clock() {
    const intervalRef = useRef<number | undefined>(undefined);
    const [time, setTime] = useState<TimeState>({
        hours: "99",
        minutes: "99",
        seconds: "99",
    });

    useEffect(() => {
        const updateClock = () => {
            const now = new Date();
            const hours = now.getUTCHours().toString().padStart(2, "0");
            const minutes = now.getUTCMinutes().toString().padStart(2, "0");
            const seconds = now.getUTCSeconds().toString().padStart(2, "0");

            setTime(prev => {
                if (prev.hours === hours && prev.minutes === minutes && prev.seconds === seconds) {
                    return prev;
                }
                return {hours, minutes, seconds};
            });

            return 1000 - now.getMilliseconds();
        };

        const diff = updateClock();
        const timeout = setTimeout(() => {
            updateClock();
            intervalRef.current = setInterval(updateClock, 1000);
        }, diff);

        return () => {
            clearTimeout(timeout);
            clearInterval(intervalRef.current);
        };
    }, []);

    return (
        <div className="h-full px-1 border-r bg-[#c3c8ce] w-min whitespace-nowrap">
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
                    {time.seconds}
                </p>
            </div>
        </div>
    );
}

export default Clock;
