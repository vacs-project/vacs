import {useRef} from "preact/hooks";
import MainPageContainer from "../components/ui/MainPageContainer.tsx";
import {invokeSafe} from "../error.ts";
import {useProfileStore} from "../stores/profile-store.ts";
import PhonePage from "./PhonePage.tsx";
import RadioPage from "./RadioPage.tsx";

const DEFAULT_PHONE_COLUMNS = 4;

const phoneWidth = (columns: number) =>
    `calc(5.5rem * ${columns} + 0.375rem * ${columns + 1} + 3px)`;

// The radio grid has a 0.375rem left padding and none on the right; the same space is kept on
// the right so the last column does not touch the border.
const phoneWidthBesideRadio = (columns: number) =>
    `calc(100% - (10rem * ${columns} + 0.5rem * ${columns - 1} + 0.75rem + 3px))`;

function SplitPage() {
    const draggingRef = useRef(false);
    const phoneContainerRightBorderRef = useRef<number | undefined>(undefined);
    const splitProfileWidth = useProfileStore(state => state.splitProfileWidth);
    const setSplitProfileWidth = useProfileStore(state => state.setSplitProfileWidth);
    const phoneColumns = useProfileStore(state => state.profile?.mixedViewPhoneColumns);
    const radioColumns = useProfileStore(state => state.profile?.mixedViewRadioColumns);

    const phoneContainerRef = (dom: HTMLDivElement | null) => {
        const bounds = dom?.getClientRects()[0];

        if (bounds !== undefined) {
            phoneContainerRightBorderRef.current = bounds.x + bounds.width;
        }

        return () => {
            phoneContainerRightBorderRef.current = undefined;
        };
    };

    const calculateAndSetWidth = (x: number) => {
        if (phoneContainerRightBorderRef.current === undefined) return;

        const zoom = parseFloat(document.documentElement.style.zoom) || 1;
        const width = Math.round((phoneContainerRightBorderRef.current - x) / zoom);
        setSplitProfileWidth(width);
    };

    const handleOnPointerDown = (e: PointerEvent) => {
        if (e.button !== 0) return;

        (e.currentTarget as HTMLDivElement).setPointerCapture(e.pointerId);
        draggingRef.current = true;
    };

    const handleOnPointerMove = (e: PointerEvent) => {
        if (!draggingRef.current) return;

        calculateAndSetWidth(e.clientX);
    };

    const handleOnDragEnd = () => {
        if (!draggingRef.current) return;
        draggingRef.current = false;

        const profileId = useProfileStore.getState().profile?.id;
        if (profileId === undefined) return;

        const width =
            splitProfileWidth !== undefined
                ? Math.min(Math.max(splitProfileWidth, 0), 65535)
                : undefined;

        void invokeSafe("app_set_split_profile_width", {profileId, width});
    };

    const handleOnDblClick = () => {
        setSplitProfileWidth(undefined);

        const profileId = useProfileStore.getState().profile?.id;

        if (profileId === undefined) return;

        void invokeSafe("app_set_split_profile_width", {profileId, width: undefined});
    };

    const width =
        splitProfileWidth !== undefined
            ? `${splitProfileWidth}px`
            : radioColumns !== undefined && phoneColumns === undefined
              ? phoneWidthBesideRadio(radioColumns)
              : phoneWidth(phoneColumns ?? DEFAULT_PHONE_COLUMNS);

    return (
        <>
            <MainPageContainer>
                <RadioPage />
            </MainPageContainer>
            <div
                className="absolute z-[5] h-full w-[calc(0.625rem+3px)] cursor-ew-resize touch-none opacity-0 bg-gray-500 hover:opacity-30 transition-opacity"
                onPointerDown={handleOnPointerDown}
                onPointerMove={handleOnPointerMove}
                onPointerUp={handleOnDragEnd}
                onPointerCancel={handleOnDragEnd}
                onDblClick={handleOnDblClick}
                style={{
                    right: `clamp(calc(5.5rem + 0.75rem - 3px), calc(${width} - 6px), calc(100% - 11.125rem - 5px))`,
                }}
            />
            <MainPageContainer
                ref={phoneContainerRef}
                width={width}
                className="min-w-[calc(5.5rem+0.75rem+3px)] max-w-[calc(100%-11.125rem+1px)]"
            >
                <PhonePage />
            </MainPageContainer>
        </>
    );
}

export default SplitPage;
