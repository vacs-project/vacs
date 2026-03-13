import {useEffect, useState} from "preact/hooks";
import {CSSProperties} from "preact";
import {invoke, isTauri} from "../transport";

const ZoomFactor = 0.05;
const BrowserZoomStorageKey = "vacs-remote-zoom-level";

export function useZoomHotkey(): CSSProperties {
    const [zoomLevel, setZoomLevel] = useState(1);

    useEffect(() => {
        if (isTauri) {
            void invoke<number>("app_get_zoom_level").then(setZoomLevel);
        } else {
            const stored = localStorage.getItem(BrowserZoomStorageKey);
            if (stored) setZoomLevel(parseFloat(stored));
        }
    }, []);

    useEffect(() => {
        const handleZoomKeyDown = async (event: KeyboardEvent) => {
            if (!(event.ctrlKey || event.metaKey)) return;

            const key = event.key;
            const code = event.code;

            let newZoom: number | undefined;
            if (key === "+" || code === "NumpadAdd") {
                newZoom = zoomLevel + ZoomFactor;
            } else if (key === "-" || code === "NumpadSubtract") {
                newZoom = Math.max(zoomLevel - ZoomFactor, ZoomFactor);
            } else if (key === "0" || code === "Digit0") {
                newZoom = 1;
            }

            if (newZoom !== undefined) {
                event.preventDefault();

                if (isTauri) {
                    newZoom = await invoke<number>("app_set_zoom_level", {zoomLevel: newZoom});
                }
                setZoomLevel(newZoom);
                if (!isTauri) {
                    localStorage.setItem(BrowserZoomStorageKey, String(newZoom));
                }
            }
        };

        document.addEventListener("keydown", handleZoomKeyDown);
        return () => document.removeEventListener("keydown", handleZoomKeyDown);
    }, [zoomLevel]);

    if (isTauri || zoomLevel === 1) return {};
    return {
        transform: `scale(${zoomLevel})`,
        transformOrigin: "top left",
        width: `${100 / zoomLevel}%`,
        height: `${100 / zoomLevel}%`,
    };
}
