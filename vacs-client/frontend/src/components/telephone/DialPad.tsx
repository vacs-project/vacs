import Button from "../ui/Button.tsx";
import {useDialPadInput} from "../../hooks/dial-pad-hook.ts";
import {clsx} from "clsx";
import {startCall} from "../../stores/call-store.ts";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {TargetedEvent} from "preact";
import {useAuthStore} from "../../stores/auth-store.ts";
import {useLastDialledClientId} from "../../stores/call-list-store.ts";
import {useConnectionStore} from "../../stores/connection-store.ts";
import {ClientId} from "../../types/generic.ts";

const DIAL_BUTTONS: {digit: string; chars: string}[] = [
    {digit: "1", chars: ""},
    {digit: "2", chars: "ABC"},
    {digit: "3", chars: "DEF"},
    {digit: "4", chars: "GHI"},
    {digit: "5", chars: "JKL"},
    {digit: "6", chars: "MNO"},
    {digit: "7", chars: "PQRS"},
    {digit: "8", chars: "TUV"},
    {digit: "9", chars: "WXYZ"},
    {digit: "*", chars: ""},
    {digit: "0", chars: ""},
    {digit: "#", chars: ""},
];

function DialPad() {
    const {dialInput, setDialInput, handleDialClick, clearLastChar, clearAll} = useDialPadInput();
    const ownId = useAuthStore(state => state.cid);
    const isDialInputEmpty = dialInput === "";
    const isDialInputOwnId = dialInput === ownId;
    const isConnected = useConnectionStore(state => state.connectionState === "connected");
    const lastDialledPeerId = useLastDialledClientId();

    const handleChange = (event: TargetedEvent<HTMLInputElement>) => {
        if (event.target instanceof HTMLInputElement) {
            const rawValue = event.target.value;

            const sanitized = rawValue
                .toUpperCase()
                .replace(/[^A-Z0-9*#]/g, "")
                .slice(0, 8);
            event.target.value = sanitized;

            setDialInput(sanitized);
        }
    };

    const handleStartCall = useAsyncDebounce(async (clientId: ClientId | undefined) => {
        if (clientId === undefined) return;
        await startCall({client: clientId});
    });

    return (
        <div className="w-full flex flex-row [&_button]:h-15 [&_button]:shrink-0 [&_button]:rounded py-3">
            <div className="flex flex-col gap-3 px-4 pt-[calc(4.5rem-1px)]">
                <Button color="blue" disabled={false}>
                    IA
                </Button>
                <Button color="cyan">
                    <p>
                        ATS
                        <br />
                        MFC
                    </p>
                </Button>
                <Button color="cyan" disabled={true} />
                <Button color="cyan" disabled={true} />
                <Button
                    color="gray"
                    disabled={!lastDialledPeerId || !isConnected}
                    onClick={() => handleStartCall(lastDialledPeerId)}
                    title={
                        !isConnected
                            ? "Disconnected"
                            : !lastDialledPeerId
                              ? "No outgoing call in call list"
                              : undefined
                    }
                >
                    Redial
                </Button>
            </div>
            <div>
                <input
                    type="text"
                    className={clsx(
                        "w-[21.75rem] h-15 px-3 rounded border border-gray-700 bg-slate-200 text-3xl font-semibold overflow-auto leading-14 mb-3",
                        "focus:border-red-500 focus:outline-none",
                    )}
                    onChange={handleChange}
                    onKeyPress={event =>
                        event.key === "Enter" && handleStartCall(dialInput as ClientId)
                    }
                    value={dialInput}
                />
                <div className="grid grid-cols-3 gap-3 [&>button]:w-27 mb-3">
                    {DIAL_BUTTONS.map(({digit, chars}, idx) => (
                        <Button
                            key={idx}
                            color="gray"
                            className="text-lg"
                            onClick={() => handleDialClick(digit, chars)}
                        >
                            {chars !== "" ? (
                                <p>
                                    {digit}
                                    <br />
                                    {chars}
                                </p>
                            ) : (
                                <p>{digit}</p>
                            )}
                        </Button>
                    ))}
                </div>
                <Button
                    color="gray"
                    className="w-full text-xl"
                    title={
                        !isConnected
                            ? "Disconnected"
                            : isDialInputOwnId
                              ? "You cannot call yourself"
                              : undefined
                    }
                    disabled={isDialInputEmpty || isDialInputOwnId || !isConnected}
                    onClick={() => handleStartCall(dialInput as ClientId)}
                >
                    Call
                </Button>
            </div>
            <div className="flex flex-col gap-3 px-4">
                <Button color="gray" disabled={isDialInputEmpty} onClick={clearLastChar}>
                    ⟵
                </Button>
                <Button color="gray" disabled={isDialInputEmpty} onClick={clearAll}>
                    <p>
                        Clear
                        <br />
                        All
                    </p>
                </Button>
            </div>
        </div>
    );
}

export default DialPad;
