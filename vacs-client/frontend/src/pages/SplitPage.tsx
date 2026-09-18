import {useEffect, useRef} from "preact/hooks";
import MainPageContainer from "../components/ui/MainPageContainer.tsx";
import {invokeSafe} from "../error.ts";
import {useEventCallback} from "../hooks/event-callback-hook.ts";
import {useProfileStore} from "../stores/profile-store.ts";
import PhonePage from "./PhonePage.tsx";
import RadioPage from "./RadioPage.tsx";

const DEFAULT_WIDTH = "calc(5.5rem * 4 + 2rem + 3px)";

function SplitPage() {
    const draggingRef = useRef(false);
    const phoneContainerRightBorderRef = useRef<number | undefined>(undefined);
    const splitProfileWidth = useProfileStore(state => state.splitProfileWidth);
    const setSplitProfileWidth = useProfileStore(state => state.setSplitProfileWidth);

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

    const handleOnMouseMove = useEventCallback((e: MouseEvent) => {
        calculateAndSetWidth(e.x);
    });

    const handleOnTouchMove = useEventCallback((e: TouchEvent) => {
        const touch = e.touches[0];
        if (!touch) return;

        calculateAndSetWidth(touch.clientX);
    });

    const handleOnDragEnd = useEventCallback(() => {
        if (draggingRef.current) {
            const profileId = useProfileStore.getState().profile?.id;

            if (profileId !== undefined) {
                const width =
                    splitProfileWidth !== undefined
                        ? Math.min(Math.max(splitProfileWidth, 0), 65535)
                        : undefined;

                void invokeSafe("app_set_split_profile_width", {profileId, width});
            }
        }

        window.removeEventListener("mousemove", handleOnMouseMove);
        window.removeEventListener("touchmove", handleOnTouchMove);
        draggingRef.current = false;
    });

    const handleOnDblClick = () => {
        setSplitProfileWidth(undefined);

        const profileId = useProfileStore.getState().profile?.id;

        if (profileId === undefined) return;

        void invokeSafe("app_set_split_profile_width", {profileId, width: undefined});
    };

    useEffect(() => {
        window.addEventListener("mouseup", handleOnDragEnd);
        window.addEventListener("touchend", handleOnDragEnd);

        return () => {
            draggingRef.current = false;
            window.removeEventListener("mousemove", handleOnMouseMove);
            window.removeEventListener("touchmove", handleOnTouchMove);
            window.removeEventListener("mouseup", handleOnDragEnd);
            window.removeEventListener("touchend", handleOnDragEnd);
        };
    }, [handleOnDragEnd, handleOnMouseMove, handleOnTouchMove]);

    const width = splitProfileWidth !== undefined ? `${splitProfileWidth}px` : DEFAULT_WIDTH;

    return (
        <>
            <MainPageContainer>
                <RadioPage />
            </MainPageContainer>
            <div
                className="absolute z-10 h-full w-[calc(0.625rem+3px)] cursor-ew-resize opacity-0 bg-gray-500 hover:opacity-30 transition-opacity"
                onMouseDown={() => {
                    draggingRef.current = true;
                    window.addEventListener("mousemove", handleOnMouseMove);
                }}
                onTouchStart={() => {
                    draggingRef.current = true;
                    window.addEventListener("touchmove", handleOnTouchMove);
                }}
                onMouseUp={handleOnDragEnd}
                onTouchEnd={handleOnDragEnd}
                onDblClick={handleOnDblClick}
                style={{
                    right: `clamp(calc(5.5rem + 0.875rem - 3px), calc(${width} - 6px), calc(100% - 11.125rem - 5px))`,
                }}
            />
            <MainPageContainer
                ref={phoneContainerRef}
                width={width}
                className="min-w-[calc(5.5rem+0.875rem+3px)] max-w-[calc(100%-11.125rem+1px)]"
            >
                <PhonePage />
            </MainPageContainer>
        </>
    );
}

export default SplitPage;
