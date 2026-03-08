import {useEffect, useRef} from "preact/hooks";
import {getCurrentWebviewWindow} from "@tauri-apps/api/webviewWindow";

const ZoomFactor = 0.05;
const ZOOM_STORAGE_KEY = "zoom-level";

export function useZoomHotkey() {
    const savedZoom = parseFloat(localStorage.getItem(ZOOM_STORAGE_KEY) ?? "1") || 1;
    const zoomRef = useRef<number>(savedZoom);

    const setZoom = async (zoom: number) => {
        await getCurrentWebviewWindow().setZoom(zoom);
        zoomRef.current = zoom;
        localStorage.setItem(ZOOM_STORAGE_KEY, String(zoom));
    };

    const handleZoomKeyDown = async (event: KeyboardEvent) => {
        if (!(event.ctrlKey || event.metaKey) || event.shiftKey) return;

        const key = event.key;
        const code = event.code;

        if (key === "+" || code === "NumpadAdd") {
            await setZoom(zoomRef.current + ZoomFactor);
        } else if (key === "-" || code === "NumpadSubtract") {
            await setZoom(zoomRef.current - ZoomFactor);
        } else if (key === "0" || code === "Digit0") {
            await setZoom(1);
        }
    };

    useEffect(() => {
        void getCurrentWebviewWindow().setZoom(zoomRef.current);

        document.addEventListener("keydown", handleZoomKeyDown);
        return () => {
            document.removeEventListener("keydown", handleZoomKeyDown);
        };
    }, []);
}
