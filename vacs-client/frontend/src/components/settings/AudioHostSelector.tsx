import Select, {SelectOption} from "../ui/Select.tsx";
import {useState, useEffect} from "preact/hooks";
import {useAsyncDebounce} from "../../hooks/debounce-hook.ts";
import {invokeStrict} from "../../error.ts";
import {AudioHosts} from "../../types/audio.ts";
import {useCallStore} from "../../stores/call-store.ts";

function AudioHostSelector() {
    const callDisplayType = useCallStore(state => state.callDisplay?.type);

    const [host, setHost] = useState<string>("");
    const [hosts, setHosts] = useState<SelectOption[]>([{value: "", text: "Loading..."}]);

    const handleOnChange = useAsyncDebounce(async (new_host: string) => {
        const previousHost = host;

        setHost(new_host);

        try {
            await invokeStrict("audio_set_host", {hostName: new_host});
        } catch {
            setHost(previousHost);
        }
    });

    useEffect(() => {
        const fetchHosts = async () => {
            try {
                const hosts = await invokeStrict<AudioHosts>("audio_get_hosts");

                setHosts(hosts.all.map(hostName => ({value: hostName, text: hostName})));
                setHost(hosts.selected);
            } catch {}
        };

        void fetchHosts();
    }, []);

    return (
        <Select
            name="audio-host"
            className="mb-1"
            options={hosts}
            selected={host}
            onChange={handleOnChange}
            disabled={
                hosts === undefined ||
                hosts.length === 0 ||
                callDisplayType === "accepted" ||
                callDisplayType === "outgoing"
            }
        />
    );
}

export default AudioHostSelector;
