import Checkbox from "../ui/Checkbox.tsx";
import {useEffect, useState} from "preact/hooks";
import {invokeStrict, invokeSafe} from "../../error.ts";
import {isTauri} from "../../transport";
import {TargetedEvent} from "preact";

function ReplaySettings() {
    const [enabled, setEnabled] = useState(false);

    useEffect(() => {
        const fetchEnabled = async () => {
            const result = await invokeSafe<boolean>("replay_get_enabled");
            if (result === undefined) return;
            setEnabled(result);
        };
        void fetchEnabled();
    }, []);

    const handleToggle = async (e: TargetedEvent<HTMLInputElement>) => {
        const next = e.currentTarget.checked;
        try {
            await invokeStrict("replay_set_enabled", {enabled: next});
            setEnabled(next);
        } catch {
            setEnabled(!next);
        }
    };

    return (
        <div className="w-full flex justify-between items-center">
            <label htmlFor="replay-enabled">Enable transmission replay</label>
            <Checkbox
                name="replay-enabled"
                checked={enabled}
                onChange={handleToggle}
                disabled={!isTauri}
            />
        </div>
    );
}

export default ReplaySettings;
