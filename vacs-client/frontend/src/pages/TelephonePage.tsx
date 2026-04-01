import {useState} from "preact/hooks";
import {clsx} from "clsx";
import {ComponentChildren} from "preact";
import "../styles/telephone-page.css";
import DialPad from "../components/telephone/DialPad.tsx";
import CallList from "../components/telephone/CallList.tsx";
import {invokeSafe} from "../error.ts";
import IgnoreList from "../components/telephone/IgnoreList.tsx";
import TelephoneDirectory from "../components/telephone/TelephoneDirectory.tsx";

type Page = "dir" | "call-list" | "dial-pad" | "ign";

const PageTitle: Record<Page, string> = {
    dir: "Telephone Directory",
    "call-list": "Call List",
    "dial-pad": "Dial Pad",
    ign: "Ignore List",
};

function TelephonePage() {
    const [page, setPage] = useState<Page>("call-list");

    return (
        <div
            className={clsx(
                "z-10 absolute h-[calc(100%+5rem+3px-0.5rem+0.25rem)] -top-px right-[-2px]",
                "bg-blue-700 px-2 pb-2 flex flex-col overflow-auto rounded-md",
                page === "dir" && "w-[calc(100%+3px)]",
            )}
        >
            <p className="w-full text-white bg-blue-700 font-semibold text-center">
                {PageTitle[page]}
            </p>
            <div className="w-full grow rounded-b-sm bg-gray-500 flex flex-row">
                <div className="grow h-full bg-[#B5BBC6] rounded-lg border-3 border-gray-600">
                    {page === "dir" ? (
                        <TelephoneDirectory />
                    ) : page === "call-list" ? (
                        <CallList />
                    ) : page === "dial-pad" ? (
                        <DialPad />
                    ) : (
                        <IgnoreList />
                    )}
                </div>
                <div className="w-19 h-full shrink-0 pt-12 flex flex-col gap-[2px]">
                    <TelephonePageButton page="dir" activePage={page} setPage={setPage}>
                        <p>Dir.</p>
                    </TelephonePageButton>
                    <TelephonePageButton page="call-list" activePage={page} setPage={setPage}>
                        <p>
                            Call
                            <br />
                            List
                        </p>
                    </TelephonePageButton>
                    <TelephonePageButton page="dial-pad" activePage={page} setPage={setPage}>
                        <p>
                            Dial
                            <br />
                            Pad
                        </p>
                    </TelephonePageButton>
                    <TelephonePageButton page="ign" activePage={page} setPage={setPage}>
                        <p>Ign.</p>
                    </TelephonePageButton>
                </div>
            </div>
        </div>
    );
}

type TelephonePageButtonProps = {
    page: Page;
    children: ComponentChildren;
    activePage: Page;
    setPage: (page: Page) => void;
};

function TelephonePageButton(props: TelephonePageButtonProps) {
    const isActive = props.activePage === props.page;

    return (
        <button
            className={clsx(
                "w-full h-16 rounded-r-lg border-2 border-l-0 font-semibold text-lg leading-5",
                "bg-[#959CA8] disabled:bg-[#B5BBC6] border-t-gray-100 border-r-gray-700 border-b-gray-700",
                "shadow-[0_-1px_0_0_#364153,1px_0_0_0_#364153,0_1px_0_0_#364153]",
                "active:border-r-gray-100 active:border-b-gray-100 active:border-t-gray-700 active:border-l-gray-700",
                "not-disabled:active:*:translate-y-px not-disabled:active:*:translate-x-px not-disabled:cursor-pointer",
                "disabled:relative disabled:border-gray-600 disabled:pointer-events-none",
                "active-telephone-page",
            )}
            onClick={() => {
                void invokeSafe("audio_play_ui_click");
                props.setPage(props.page);
            }}
            disabled={isActive}
        >
            {props.children}
        </button>
    );
}

export default TelephonePage;
