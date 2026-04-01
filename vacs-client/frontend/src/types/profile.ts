import {ProfileId, StationId} from "./generic.ts";
import {ClientPageConfig} from "./client.ts";
import {CustomButtonColor} from "./custom-button-colors.ts";

export type SessionProfile = {
    type: "changed" | "unchanged";
    activeProfile?: ActiveProfile;
};

export type ActiveProfile = {
    type: "specific" | "custom" | "none";
    profile?: Profile;
};

export type Profile = {
    id: ProfileId;
    geo?: GeoPageContainer;
    tabbed?: Tab[];
};

export type Tab = {
    label: string[];
    page: DirectAccessPage;
};

export type GeoPageContainer = {
    height?: string; // "<number>['%' | 'rem']"
    width?: string; // "<number>['%' | 'rem']"
    padding?: number; // in rem
    paddingLeft?: number; // in rem
    paddingRight?: number; // in rem
    paddingTop?: number; // in rem
    paddingBottom?: number; // in rem
    gap?: number; // in rem
    justifyContent?: "start" | "end" | "space-between" | "space-around" | "space-evenly" | "center";
    alignItems?: "start" | "end" | "center";
    direction: "row" | "col";
    children: (GeoPageContainer | GeoPageButton | GeoPageDivider)[];
};

export type GeoPageButton = {
    label: string[];
    size: number; // size in rem, > 0
    color?: CustomButtonColor;
    page?: DirectAccessPage;
    stationId?: StationId;
};

export type GeoPageDivider = {
    orientation: "horizontal" | "vertical";
    thickness: number; // size in px, > 0
    color: string;
    oversize?: number; // in rem, > 0
};

export type DirectAccessPage = {
    keys?: DirectAccessKey[];
    clientPage?: ClientPageConfig;
    rows: number; // > 0
};

export type DirectAccessKey = {
    label: string[];
    color?: CustomButtonColor;
    stationId?: StationId;
    page?: DirectAccessPage;
};

export function isGeoPageContainer(container: unknown): container is GeoPageContainer {
    if (typeof container !== "object" || container === null) {
        return false;
    }

    const maybeContainer = container as Record<string, unknown>;

    return typeof maybeContainer.direction === "string" && Array.isArray(maybeContainer.children);
}

export function isGeoPageDivider(divider: unknown): divider is GeoPageDivider {
    if (typeof divider !== "object" || divider === null) {
        return false;
    }

    const maybeDivider = divider as Record<string, unknown>;

    return (
        typeof maybeDivider.orientation === "string" &&
        typeof maybeDivider.thickness === "number" &&
        (maybeDivider.color === undefined || typeof maybeDivider.color === "string")
    );
}

export function isGeoPageButton(button: unknown): button is GeoPageButton {
    if (typeof button !== "object" || button === null) {
        return false;
    }

    const maybeButton = button as Record<string, unknown>;

    return (
        Array.isArray(maybeButton.label) &&
        typeof maybeButton.size === "number" &&
        (maybeButton.page === undefined || typeof maybeButton.page === "object")
    );
}
