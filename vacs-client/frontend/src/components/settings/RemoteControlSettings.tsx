import Checkbox from "../ui/Checkbox.tsx";
import {StatusColors} from "../ui/StatusIndicator.tsx";
import {useEffect, useState} from "preact/hooks";
import {invokeStrict, invokeSafe} from "../../error.ts";
import {listen} from "../../transport";
import {clsx} from "clsx";
import {parseSocketAddress} from "../../utils/socket-address.ts";
import {RemoteConfig, RemoteStatus} from "../../types/settings.ts";
import {TargetedEvent} from "preact";

const DEFAULT_IP = "0.0.0.0";
const DEFAULT_PORT = 9600;

function RemoteControlSettings() {
    const [config, setConfig] = useState<RemoteConfig | undefined>(undefined);
    const [listenAddr, setListenAddr] = useState<string>("");
    const [addrError, setAddrError] = useState(false);
    const [status, setStatus] = useState<RemoteStatus>({listening: false, connectedClients: 0});

    useEffect(() => {
        const fetchConfig = async () => {
            const result = await invokeSafe<RemoteConfig & RemoteStatus>("remote_get_config");
            if (result) {
                setConfig(result);
                setListenAddr(result.listenAddr);
                setStatus(result);
            }
        };
        void fetchConfig();

        const unlisten = listen<RemoteStatus>("remote:status", event => {
            setStatus(event.payload);
        });

        return () => {
            unlisten.then(fn => fn());
        };
    }, []);

    if (config === undefined) return null;

    const handleToggleEnabled = async (e: TargetedEvent<HTMLInputElement>) => {
        const enabled = e.currentTarget.checked;
        const newConfig = {...config, enabled};
        try {
            await invokeStrict("remote_set_config", {remoteConfig: newConfig});
            setConfig(newConfig);
        } catch {
            setConfig({...config, enabled: !enabled});
        }
    };

    const handleAddrChange = (e: TargetedEvent<HTMLInputElement>) => {
        setListenAddr(e.currentTarget.value);
        setAddrError(false);
    };

    const handleAddrCommit = async () => {
        const addr = parseSocketAddress(listenAddr, DEFAULT_IP, DEFAULT_PORT);

        if (addr === null) {
            setAddrError(true);
            return;
        }

        setListenAddr(addr);
        setAddrError(false);

        if (addr === config.listenAddr) return;

        const newConfig = {...config, listenAddr: addr};
        try {
            await invokeStrict("remote_set_config", {remoteConfig: newConfig});
            setConfig(newConfig);
        } catch {
            setListenAddr(config.listenAddr);
        }
    };

    return (
        <>
            <div className="w-full flex justify-between items-center">
                <label htmlFor="remote-enabled">Enable remote control</label>
                <Checkbox
                    name="remote-enabled"
                    checked={config.enabled}
                    onChange={handleToggleEnabled}
                />
            </div>
            <div className="w-full flex justify-between items-center gap-3">
                <label htmlFor="remote-listen-addr" className="shrink-0">
                    Listen address
                </label>
                <div className="flex flex-row gap-2 items-center">
                    <input
                        type="text"
                        id="remote-listen-addr"
                        className={clsx(
                            "w-full h-full px-3 py-1.5 border bg-gray-300 rounded text-sm text-center focus:outline-none placeholder:text-gray-500",
                            "disabled:brightness-90 disabled:cursor-not-allowed",
                            addrError
                                ? "border-red-500 focus:border-red-500"
                                : "border-gray-700 focus:border-blue-500",
                        )}
                        placeholder={`${DEFAULT_IP}:${DEFAULT_PORT}`}
                        title={
                            addrError
                                ? `Invalid address. Accepted formats: "1.2.3.4", "1.2.3.4:port", "::1", "[::1]:port"`
                                : `Address to listen on. Omit port to use the default (${DEFAULT_PORT}). Leave empty to reset to ${DEFAULT_IP}:${DEFAULT_PORT}.`
                        }
                        value={listenAddr}
                        onInput={handleAddrChange}
                        onBlur={handleAddrCommit}
                        onKeyDown={e => {
                            if (e.key === "Enter") e.currentTarget.blur();
                        }}
                        disabled={!config.enabled}
                    />
                    <RemoteStatusIndicator status={status} enabled={config.enabled} />
                </div>
            </div>
        </>
    );
}

function RemoteStatusIndicator(props: {status: RemoteStatus; enabled: boolean}) {
    const {status, enabled} = props;

    let color: string;
    let title: string;

    if (!enabled) {
        color = StatusColors["gray"];
        title = "Disabled";
    } else if (!status.listening) {
        color = StatusColors["red"];
        title = "Not listening";
    } else if (status.connectedClients > 0) {
        color = StatusColors["green"];
        title = `Listening - ${status.connectedClients} client${status.connectedClients !== 1 ? "s" : ""} connected`;
    } else {
        color = StatusColors["blue"];
        title = "Listening - no clients connected";
    }

    return <div className={clsx("shrink-0 h-3 w-3 rounded-full border", color)} title={title} />;
}

export default RemoteControlSettings;
