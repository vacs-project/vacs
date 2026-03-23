import TransmitModeSettings from "./TransmitModeSettings.tsx";
import SettingsSubPage from "./SettingsSubPage.tsx";

function TransmitModePage() {
    return (
        <SettingsSubPage
            title="Transmit Config"
            width="w-[69%]"
            className="flex flex-col overflow-y-auto"
        >
            <TransmitModeSettings />
        </SettingsSubPage>
    );
}

export default TransmitModePage;
