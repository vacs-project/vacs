import {clsx} from "clsx";
import {closeMenu, setPage, useNavigationStore} from "../../stores/navigation-store.ts";
import {retryRadioConnection} from "../../stores/radio-store.ts";
import Button from "./Button.tsx";

function PageCycleButton() {
    const page = useNavigationStore(state => state.page);
    const settingsOpen = useNavigationStore(state => state.menu === "settings");

    return (
        <Button
            color="gray"
            className="w-22"
            onClick={() => {
                const next = page === "radio" ? "phone" : page === "phone" ? "split" : "radio";
                setPage(next);
                if (settingsOpen) closeMenu();
                if (next !== "phone") retryRadioConnection();
            }}
        >
            <div className="w-full h-full pt-1 flex flex-col justify-between items-center">
                <p>Page</p>
                <div className="w-full border-t flex flex-row justify-around *:not-last:border-r *:flex-1 *:text-center *:py-0.5">
                    <p className={clsx(page === "radio" && "bg-gray-400")}>R</p>
                    <p className={clsx(page === "phone" && "bg-gray-400")}>P</p>
                    <p className={clsx(page === "split" && "bg-gray-400")}>M</p>
                </div>
            </div>
        </Button>
    );
}

export default PageCycleButton;
