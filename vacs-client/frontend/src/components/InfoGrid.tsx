import {useAuthStore} from "../stores/auth-store.ts";
import "../styles/info-grid.css";
import {useCallStore} from "../stores/call-store.ts";
import {useUpdateStore} from "../stores/update-store.ts";
import {navigate} from "wouter/use-browser-location";
import {invokeSafe} from "../error.ts";
import {clsx} from "clsx";
import {isTauri} from "../transport";
import {useConnectionStore} from "../stores/connection-store.ts";

async function openUrl(url: string): Promise<void> {
    if (isTauri) {
        const mod = await import("@tauri-apps/plugin-opener");
        await mod.openUrl(url);
    } else {
        window.open(url, "_blank");
    }
}

function InfoGrid() {
    const cid = useAuthStore(state => state.cid);
    const clientInfo = useConnectionStore(state =>
        state.connectionState === "connected"
            ? `${state.info.positionId || state.info.displayName || ""}${state.info.frequency !== "" ? ` (${state.info.frequency})` : ""}`
            : "",
    );
    const callErrorReason = useCallStore(state => state.callDisplay?.errorReason);
    const currentVersion = useUpdateStore(state => state.currentVersion);
    const newVersion = useUpdateStore(state => state.newVersion);

    const currentVersionText = `Version: v${currentVersion}`;
    const updateAvailableText = newVersion !== undefined ? `UPDATE AVAILABLE (v${newVersion})` : "";

    const handleVersionClick = async (version: string) => {
        await openUrl(`https://github.com/vacs-project/vacs/releases/tag/vacs-client-v${version}`);
        void invokeSafe("audio_play_ui_click");
        navigate("/settings");
    };

    return (
        <div
            className="grid grid-rows-2 w-full h-full"
            style={{gridTemplateColumns: "25% 32.5% 42.5%"}}
        >
            <div className="info-grid-cell" title={cid}>
                {cid}
            </div>
            <div
                className="info-grid-cell cursor-pointer"
                title={currentVersionText}
                onClick={() => handleVersionClick(currentVersion)}
            >
                {currentVersionText}
            </div>
            <div className="info-grid-cell"></div>
            <div className="info-grid-cell" title={clientInfo}>
                {clientInfo}
            </div>
            <div
                className={clsx("info-grid-cell", newVersion !== undefined && "cursor-pointer")}
                title={updateAvailableText}
                onClick={() => newVersion !== undefined && handleVersionClick(newVersion)}
            >
                {updateAvailableText}
            </div>
            <div className="info-grid-cell uppercase" title={callErrorReason}>
                {callErrorReason}
            </div>
        </div>
    );
}

export default InfoGrid;
