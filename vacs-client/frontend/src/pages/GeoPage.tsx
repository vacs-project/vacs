import Button from "../components/ui/Button.tsx";
import {
    GeoPageButton as GeoPageButtonModel,
    GeoPageContainer as GeoPageContainerModel,
    GeoPageDivider as GeoPageDividerModel,
    isGeoPageButton,
    isGeoPageContainer,
    isGeoPageDivider,
} from "../types/profile.ts";
import {CSSProperties} from "preact";
import {useProfileStore} from "../stores/profile-store.ts";
import DirectAccessPage from "../components/DirectAccessPage.tsx";
import {clsx} from "clsx";
import ButtonLabel from "../components/ui/ButtonLabel.tsx";
import {useCallState} from "../hooks/call-state-hook.ts";
import {useStationKeyInteraction} from "../hooks/station-key-interaction-hook.ts";

type GeoPageProps = {
    page: GeoPageContainerModel;
};

function GeoPage({page}: GeoPageProps) {
    const selectedPage = useProfileStore(state => state.page.current);

    return selectedPage !== undefined ? (
        <DirectAccessPage data={selectedPage} />
    ) : (
        <GeoPageContainer
            className="w-full h-full [&_div]:min-w-min [&_div]:min-h-min overflow-auto"
            container={page}
        />
    );
}

function GeoPageContainer({
    container,
    className,
}: {
    container: GeoPageContainerModel;
    className?: string;
}) {
    const style: CSSProperties = {
        height: container.height,
        width: container.width,
        display: "flex",
        flexDirection: container.direction === "row" ? "row" : "column",
        justifyContent: container.justifyContent,
        alignItems: container.alignItems,
        gap: container.gap && `${container.gap}rem`,
    };

    if (container.padding !== undefined) {
        style.padding = `${container.padding}rem`;
    }
    if (container.paddingLeft !== undefined) {
        style.paddingLeft = `${container.paddingLeft}rem`;
    }
    if (container.paddingRight !== undefined) {
        style.paddingRight = `${container.paddingRight}rem`;
    }
    if (container.paddingTop !== undefined) {
        style.paddingTop = `${container.paddingTop}rem`;
    }
    if (container.paddingBottom !== undefined) {
        style.paddingBottom = `${container.paddingBottom}rem`;
    }

    if (container.alignItems !== undefined) {
        let alignItems: string = container.alignItems;

        if (container.alignItems === "start") {
            alignItems = "flex-start";
        } else if (container.alignItems === "end") {
            alignItems = "flex-end";
        }

        style.alignItems = alignItems;
    }

    return (
        <div className={className} style={style}>
            {container.children.map((child, index) => {
                if (isGeoPageContainer(child)) {
                    return <GeoPageContainer container={child} key={index} />;
                } else if (isGeoPageDivider(child)) {
                    return <GeoPageDivider divider={child} key={index} />;
                } else if (isGeoPageButton(child)) {
                    return <GeoPageButton button={child} key={index} />;
                }
            })}
        </div>
    );
}

function GeoPageButton({button}: {button: GeoPageButtonModel}) {
    const hasStationId = button.stationId !== undefined;

    const {
        color: stationColor,
        highlight: stationHighlight,
        disabled,
        own,
        handleClick,
    } = useStationKeyInteraction(button.stationId, button.color);

    const {color: groupColor, highlight: groupHighlight} = useCallState(button.page, button.color);
    const setSelectedPage = useProfileStore(state => state.setPage);

    const color = hasStationId ? stationColor : groupColor;

    return (
        <Button
            color={color}
            highlight={hasStationId ? stationHighlight : groupHighlight}
            disabled={hasStationId && disabled}
            className={clsx(
                "aspect-square w-auto! rounded-none! overflow-hidden",
                color === "gray" ? "p-1.5" : "p-[calc(0.375rem+1px)]",
                hasStationId && own ? "text-gray-500" : undefined,
            )}
            style={{height: `${button.size}rem`}}
            onClick={() => {
                if (hasStationId) {
                    void handleClick();
                } else {
                    setSelectedPage(button.page);
                }
            }}
        >
            <ButtonLabel label={button.label} />
        </Button>
    );
}

type GeoPageDividerProps = {
    divider: GeoPageDividerModel;
};

function GeoPageDivider({divider}: GeoPageDividerProps) {
    return (
        <div
            className="relative"
            style={{
                height: divider.orientation === "vertical" ? "100%" : `${divider.thickness}px`,
                width: divider.orientation === "horizontal" ? "100%" : `${divider.thickness}px`,
            }}
        >
            <div
                className="absolute w-full"
                style={{
                    backgroundColor: divider.color,
                    top: divider.oversize && `-${divider.oversize}rem`,
                    height: divider.oversize ? `calc(100% + ${divider.oversize * 2}rem)` : "100%",
                }}
            ></div>
        </div>
    );
}

export default GeoPage;
