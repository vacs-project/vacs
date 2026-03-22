import {CloseButton} from "../../pages/SettingsPage.tsx";
import RemoteControlSettings from "./RemoteControlSettings.tsx";

function AdvancedPage() {
    return (
        <div className="absolute top-0 z-10 h-full w-2/5 bg-blue-700 border-t-0 px-2 pb-2 flex flex-col">
            <p className="w-full text-white bg-blue-700 font-semibold text-center">Advanced</p>
            <div className="w-full grow rounded-b-sm bg-[#B5BBC6] flex flex-col overflow-auto">
                <div className="w-full grow border-b-2 border-zinc-200 flex flex-col overflow-y-auto">
                    <p className="w-full text-center border-b-2 border-zinc-200 uppercase font-semibold">
                        Remote Control
                    </p>
                    <div className="w-full py-3 px-4 flex flex-col gap-3">
                        <RemoteControlSettings />
                    </div>
                </div>
                <div className="h-20 w-full shrink-0 flex flex-row gap-2 justify-end p-2 [&>button]:px-1 [&>button]:shrink-0 overflow-x-auto scrollbar-hide">
                    <CloseButton />
                </div>
            </div>
        </div>
    );
}

export default AdvancedPage;
