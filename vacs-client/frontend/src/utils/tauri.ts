import {invoke, isTauri} from "../transport";

export async function openUrl(url: string): Promise<void> {
    if (isTauri) {
        // Deliberately not the opener plugin: inside an AppImage the bundled xdg-open shadows the
        // host one and silently does nothing. app_open_url cleans the bundle out of the child
        // environment first.
        await invoke("app_open_url", {url});
    } else {
        window.open(url, "_blank");
    }
}
