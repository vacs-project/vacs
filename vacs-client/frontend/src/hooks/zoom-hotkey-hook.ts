import {useEffect, useRef} from "preact/hooks";
import {isTauri} from "../transport";

const ZoomFactor = 0.05;

export function useZoomHotkey() {
    const zoomRef = useRef<number>(1);

    const handleZoomKeyDown = async (event: KeyboardEvent) => {
        if (!isTauri) return; // Zoom hotkeys only work in native Tauri windows
        if (!(event.ctrlKey || event.metaKey) || event.shiftKey) return;

        const key = event.key;
        const code = event.code;

        const {getCurrentWebviewWindow} = await import("@tauri-apps/api/webviewWindow");

        if (key === "+" || code === "NumpadAdd") {
            await getCurrentWebviewWindow().setZoom(zoomRef.current + ZoomFactor);
            zoomRef.current += ZoomFactor;
        } else if (key === "-" || code === "NumpadSubtract") {
            await getCurrentWebviewWindow().setZoom(zoomRef.current - ZoomFactor);
            zoomRef.current -= ZoomFactor;
        } else if (key === "0" || code === "Digit0") {
            await getCurrentWebviewWindow().setZoom(1);
            zoomRef.current = 1;
        }
    };

    useEffect(() => {
        document.addEventListener("keydown", handleZoomKeyDown);

        return () => {
            document.removeEventListener("keydown", handleZoomKeyDown);
        };
    }, []);
}
