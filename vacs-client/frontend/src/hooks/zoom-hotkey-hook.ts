import {useEffect} from "preact/hooks";
import {invoke, isTauri} from "../transport";

const ZoomFactor = 0.05;
const BrowserZoomStorageKey = "vacs-remote-zoom-level";

type ZoomChange = "increase" | "decrease" | "reset";

export function useZoomHotkey() {
    async function handleZoomKeyDown(event: KeyboardEvent) {
        if (!(event.ctrlKey || event.metaKey)) return;

        const key = event.key;
        const code = event.code;

        let change: ZoomChange | undefined;
        if (key === "+" || code === "NumpadAdd") {
            change = "increase";
        } else if (key === "-" || code === "NumpadSubtract") {
            change = "decrease";
        } else if (key === "0" || code === "Digit0") {
            change = "reset";
        }

        if (change !== undefined) {
            event.preventDefault();

            if (isTauri) {
                void invoke("app_change_zoom_level", {change});
            } else {
                const stored = localStorage.getItem(BrowserZoomStorageKey);
                let current = stored !== null ? parseFloat(stored) : 1.0;

                switch (change) {
                    case "increase":
                        current += ZoomFactor;
                        break;
                    case "decrease":
                        current = Math.max(current - ZoomFactor, ZoomFactor);
                        break;
                    case "reset":
                        current = 1.0;
                        break;
                }

                const s = current.toString();
                localStorage.setItem(BrowserZoomStorageKey, s);
                document.documentElement.style.zoom = s;
            }
        }
    }

    useEffect(() => {
        if (!isTauri) {
            const stored = localStorage.getItem(BrowserZoomStorageKey);
            if (stored) {
                document.documentElement.style.zoom = stored;
            }
        }

        document.addEventListener("keydown", handleZoomKeyDown);
        return () => document.removeEventListener("keydown", handleZoomKeyDown);
    }, []);
}
