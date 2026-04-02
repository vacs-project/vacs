import RemoteControlSettings from "./RemoteControlSettings.tsx";
import SettingsSubPage from "./SettingsSubPage.tsx";
import AudioHostSelector from "./AudioHostSelector.tsx";

function AdvancedPage() {
    return (
        <SettingsSubPage title="Advanced" width="w-[45%]" className="flex flex-col overflow-y-auto">
            <p className="w-full text-center border-b-2 border-zinc-200 uppercase font-semibold">
                Remote Control
            </p>
            <div className="w-full py-3 px-4 flex flex-col gap-3">
                <RemoteControlSettings />
            </div>
            <p className="w-full text-center border-t-2 pt-1 border-zinc-200 uppercase font-semibold">
                Audio Backend
            </p>
            <div className="w-full py-2 px-4 border-b-2 border-zinc-200 flex flex-col gap-3">
                <AudioHostSelector />
            </div>
        </SettingsSubPage>
    );
}

export default AdvancedPage;
