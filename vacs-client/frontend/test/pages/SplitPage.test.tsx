import {afterEach, beforeEach, describe, expect, it, vi} from "vitest";
import {cleanup, fireEvent, render} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import SplitPage from "../../src/pages/SplitPage.tsx";
import {useProfileStore} from "../../src/stores/profile-store.ts";
import {ProfileId} from "../../src/types/generic.ts";
import {Profile} from "../../src/types/profile.ts";

const PROFILE: Profile = {
    id: "profile0" as ProfileId,
    view: "split",
    tabbed: [{label: ["A"], page: {rows: 1}}],
};

function mockPhoneContainerBounds(bounds: {x: number; width: number}) {
    vi.spyOn(Element.prototype, "getClientRects").mockReturnValue([
        bounds,
    ] as unknown as DOMRectList);
}

function getHandle(container: Element) {
    const handle = container.querySelector<HTMLElement>(".cursor-ew-resize");
    if (handle === null) throw new Error("split handle not found");
    return handle;
}

const setPointerCapture = vi.fn<(pointerId: number) => void>();

beforeEach(() => {
    Element.prototype.setPointerCapture = setPointerCapture;
});

afterEach(() => {
    useProfileStore.getState().reset();
    vi.restoreAllMocks();
    vi.clearAllMocks();
    cleanup();
});

describe("SplitPage", () => {
    it("captures the pointer and persists the dragged width for the profile", () => {
        useProfileStore.getState().setProfile(PROFILE, undefined);
        mockPhoneContainerBounds({x: 100, width: 200});
        const {container} = render(<SplitPage />);
        const handle = getHandle(container);

        fireEvent.pointerDown(handle, {button: 0, pointerId: 1});
        fireEvent.pointerMove(handle, {pointerId: 1, clientX: 250});
        fireEvent.pointerUp(handle, {pointerId: 1});

        expect(setPointerCapture).toHaveBeenCalledWith(1);
        expect(invoke).toHaveBeenCalledWith("app_set_split_profile_width", {
            profileId: "profile0",
            width: 50,
        });
    });

    it("clamps a computed width above 65535 before persisting", () => {
        useProfileStore.getState().setProfile(PROFILE, undefined);
        mockPhoneContainerBounds({x: 70000, width: 0});
        const {container} = render(<SplitPage />);
        const handle = getHandle(container);

        fireEvent.pointerDown(handle, {button: 0, pointerId: 1});
        fireEvent.pointerMove(handle, {pointerId: 1, clientX: 0});
        fireEvent.pointerUp(handle, {pointerId: 1});

        expect(invoke).toHaveBeenCalledWith("app_set_split_profile_width", {
            profileId: "profile0",
            width: 65535,
        });
    });

    it("persists on pointercancel just like pointerup", () => {
        useProfileStore.getState().setProfile(PROFILE, undefined);
        mockPhoneContainerBounds({x: 100, width: 200});
        const {container} = render(<SplitPage />);
        const handle = getHandle(container);

        fireEvent.pointerDown(handle, {button: 0, pointerId: 1});
        fireEvent.pointerMove(handle, {pointerId: 1, clientX: 260});
        fireEvent.pointerCancel(handle, {pointerId: 1});

        expect(invoke).toHaveBeenCalledWith("app_set_split_profile_width", {
            profileId: "profile0",
            width: 40,
        });
    });

    it("ignores a drag started with a secondary button", () => {
        useProfileStore.getState().setProfile(PROFILE, undefined);
        mockPhoneContainerBounds({x: 100, width: 200});
        const {container} = render(<SplitPage />);
        const handle = getHandle(container);

        fireEvent.pointerDown(handle, {button: 2, pointerId: 1});
        fireEvent.pointerMove(handle, {pointerId: 1, clientX: 250});
        fireEvent.pointerUp(handle, {pointerId: 1});

        expect(setPointerCapture).not.toHaveBeenCalled();
        expect(useProfileStore.getState().splitProfileWidth).toBeUndefined();
        expect(invoke.mock.calls.map(call => call[0])).not.toContain("app_set_split_profile_width");
    });

    it("does not persist a pointerup without a preceding pointerdown", () => {
        useProfileStore.getState().setProfile(PROFILE, undefined);
        mockPhoneContainerBounds({x: 100, width: 200});
        const {container} = render(<SplitPage />);
        const handle = getHandle(container);

        fireEvent.pointerUp(handle, {pointerId: 1});

        expect(invoke.mock.calls.map(call => call[0])).not.toContain("app_set_split_profile_width");
    });

    it("resets the width on double click", () => {
        useProfileStore.getState().setProfile(PROFILE, 400);
        mockPhoneContainerBounds({x: 100, width: 200});
        const {container} = render(<SplitPage />);
        const handle = getHandle(container);

        fireEvent.dblClick(handle);

        expect(invoke).toHaveBeenCalledWith("app_set_split_profile_width", {
            profileId: "profile0",
            width: undefined,
        });
        expect(useProfileStore.getState().splitProfileWidth).toBeUndefined();
    });
});
