import {clsx} from "clsx";
import {useUpdateStore} from "../../stores/update-store.ts";
import Button from "../ui/Button.tsx";
import {useEffect, useRef, useState} from "preact/hooks";
import {useAsyncDebounceState} from "../../hooks/debounce-hook.ts";
import {listen, UnlistenFn, isTauri, invoke} from "../../transport";
import {invokeStrict} from "../../error.ts";

async function closeWindow(): Promise<void> {
    if (isTauri) {
        const mod = await import("@tauri-apps/api/window");
        await mod.getCurrentWindow().close();
    }
}

function UpdateOverlay() {
    const overlayVisible = useUpdateStore(state => state.overlayVisible);
    const mandatoryDialogVisible = useUpdateStore(state => state.mandatoryDialogVisible);
    const downloadDialogVisible = useUpdateStore(state => state.downloadDialogVisible);
    const newVersion = useUpdateStore(state => state.newVersion);
    const {
        setVersions: setUpdateVersions,
        openMandatoryDialog,
        openDownloadDialog,
        closeOverlay,
    } = useUpdateStore(state => state.actions);

    const unlistenFns = useRef<Promise<UnlistenFn>[]>([]);
    const [downloadProgress, setDownloadProgress] = useState<number>(0);

    const [handleUpdateClick, updating] = useAsyncDebounceState(async () => {
        try {
            openDownloadDialog();
            await invokeStrict("app_update");
        } catch {
            openMandatoryDialog();
        }
    });

    useEffect(() => {
        const checkForUpdate = async () => {
            try {
                const checkUpdateResult = await invokeStrict<{
                    currentVersion: string;
                    newVersion?: string;
                    required: boolean;
                }>("app_check_for_update");

                setUpdateVersions(checkUpdateResult.currentVersion, checkUpdateResult.newVersion);

                if (checkUpdateResult.required) {
                    openMandatoryDialog();
                } else {
                    closeOverlay();
                }
            } catch {
                setUpdateVersions(await invoke("app_get_version"));
                closeOverlay();
            }
        };
        void checkForUpdate();
    }, [closeOverlay, openMandatoryDialog, setUpdateVersions]);

    useEffect(() => {
        if (downloadDialogVisible) {
            unlistenFns.current.push(
                listen<number>("update:progress", event => {
                    setDownloadProgress(event.payload);
                }),
            );
        } else {
            unlistenFns.current.forEach(f => f.then(fn => fn()));
        }
    }, [downloadDialogVisible]);

    useEffect(() => {
        if (overlayVisible) {
            document.addEventListener("keydown", preventKeyDown);
        } else {
            document.removeEventListener("keydown", preventKeyDown);
        }
    }, [overlayVisible]);

    return overlayVisible ? (
        <div
            className={clsx(
                "z-40 absolute top-0 left-0 w-full h-full flex justify-center items-center",
                (mandatoryDialogVisible || downloadDialogVisible) && "bg-[rgba(0,0,0,0.5)]",
            )}
        >
            {mandatoryDialogVisible && (
                <div className="bg-gray-300 border-4 border-t-blue-500 border-l-blue-500 border-b-blue-700 border-r-blue-700 rounded w-100 py-2">
                    <p className="w-full text-center text-lg font-semibold wrap-break-word">
                        Mandatory update
                    </p>
                    <p className="w-full text-center wrap-break-word mb-2">
                        In order to continue using VACS, you will need to update to version v
                        {newVersion}.<br />
                        Do you want to download and install the update?
                        <br />
                        This will restart the application.
                    </p>
                    <div
                        className={clsx(
                            "w-full flex flex-row gap-2 justify-center items-center mb-2",
                            updating && "brightness-90 [&>button]:cursor-not-allowed",
                        )}
                    >
                        <Button
                            color="red"
                            className="px-3 py-1"
                            muted={true}
                            disabled={updating}
                            onClick={() => void closeWindow()}
                        >
                            Quit
                        </Button>
                        <Button
                            color="green"
                            className="px-3 py-1"
                            onClick={handleUpdateClick}
                            disabled={updating}
                        >
                            Update
                        </Button>
                    </div>
                    {updating && <p className="w-full text-center font-semibold">Updating...</p>}
                </div>
            )}
            {downloadDialogVisible && (
                <div className="bg-gray-300 border-4 border-t-blue-500 border-l-blue-500 border-b-blue-700 border-r-blue-700 rounded w-100 py-2 px-5">
                    <p className="w-full text-center text-lg font-semibold wrap-break-word">
                        Updating...
                    </p>
                    <p className="w-full text-center wrap-break-word mb-4">
                        The application will restart once the update is downloaded and installed.
                    </p>
                    <div className="w-full h-1.5 rounded-full bg-gray-400">
                        <div
                            className="h-full rounded bg-green-600"
                            style={{width: `${downloadProgress}%`}}
                        ></div>
                    </div>
                    <p className="w-full text-sm text-gray-600">
                        {downloadProgress < 100 ? "Downloading" : "Installing"}...
                    </p>
                </div>
            )}
        </div>
    ) : (
        <></>
    );
}

const preventKeyDown = (event: KeyboardEvent) => {
    event.preventDefault();
};

export default UpdateOverlay;
