import {setPage, useNavigationStore} from "../stores/navigation-store.ts";
import {retryRadioConnection} from "../stores/radio-store.ts";
import TabButton from "./ui/TabButton.tsx";

function PageTabs() {
    const page = useNavigationStore(state => state.page);
    const settingsOpen = useNavigationStore(state => state.menu === "settings");

    return (
        <div className="h-full flex flex-row">
            <TabButton
                label={["Phone"]}
                onClick={() => setPage("phone")}
                active={page === "phone"}
                tabViewHidden={settingsOpen}
            />
            <TabButton
                label={["Radio"]}
                onClick={() => {
                    setPage("split");
                    retryRadioConnection();
                }}
                active={page === "split" || page === "radio"}
                tabViewHidden={settingsOpen}
            />
        </div>
    );
}

export default PageTabs;
