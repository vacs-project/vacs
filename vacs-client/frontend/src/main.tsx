import {render} from "preact";
import App from "./App";
import "./styles/main.css";
import {logError, safeSerialize} from "./error.ts";

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
