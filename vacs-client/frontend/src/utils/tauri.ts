import {invoke, isTauri} from "../transport";

export async function openUrl(url: string): Promise<void> {
    if (isTauri) {
        // Goes through external::open_url on the backend: inside an AppImage the child would
        // inherit the bundle's LD_LIBRARY_PATH and the host opener fails to load.
        await invoke("app_open_url", {url});
    } else {
        window.open(url, "_blank", "noopener,noreferrer");
    }
}
