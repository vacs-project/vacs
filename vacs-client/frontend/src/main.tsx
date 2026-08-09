import {render} from "preact";
import App from "./App";
import "./styles/main.css";
import {logError, safeSerialize} from "./error.ts";
import {isTauri} from "./transport";

// WebDriver automation hooks for E2E builds. The mode check is statically
// eliminated in production builds, dropping the import and its chunk; the
// isTauri guard keeps it out of remote-control browser sessions, which are
// served this same bundle over HTTP.
if (import.meta.env.MODE === "e2e" && isTauri) {
    await import("@wdio/tauri-plugin");
    (await import("./e2e-hooks.ts")).installE2eHooks();
}

window.addEventListener("error", ev => {
    logError(
        `Webview error: ${JSON.stringify({
            filename: ev.filename,
            lineno: ev.lineno,
            colno: ev.colno,
            error: safeSerialize(ev.error),
        })}`,
    );
});
window.addEventListener("unhandledrejection", ev => {
    logError(`Unhandled webview rejection: ${JSON.stringify(safeSerialize(ev.reason))}`);
});

render(<App />, document.getElementById("root")!);
