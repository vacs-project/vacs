import {afterEach, describe, expect, it, vi} from "vitest";
import {cleanup, fireEvent, render, screen} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import PageTabs from "../../src/components/PageTabs.tsx";
import {useNavigationStore} from "../../src/stores/navigation-store.ts";
import {useRadioStore} from "../../src/stores/radio-store.ts";

afterEach(() => {
    useNavigationStore.setState({
        page: "phone",
        menu: undefined,
        submenu: undefined,
        previous: {page: "phone", menu: undefined, submenu: undefined},
    });
    useRadioStore.setState({radioState: undefined});
    vi.clearAllMocks();
    cleanup();
});

describe("PageTabs", () => {
    it("switches to the split page and reconnects when the radio is disconnected", () => {
        useRadioStore.setState({radioState: {state: "Disconnected"}});
        render(<PageTabs />);

        fireEvent.click(screen.getByRole("button", {name: "Radio"}));

        expect(useNavigationStore.getState().page).toBe("split");
        expect(invoke).toHaveBeenCalledWith("radio_reconnect", undefined);
    });

    it("switches to the split page but does not reconnect an already connected radio", () => {
        useRadioStore.setState({radioState: {state: "Connected"}});
        render(<PageTabs />);

        fireEvent.click(screen.getByRole("button", {name: "Radio"}));

        expect(useNavigationStore.getState().page).toBe("split");
        expect(invoke.mock.calls.map(call => call[0])).not.toContain("radio_reconnect");
    });

    it("reconnects the radio when it is in an error state", () => {
        useRadioStore.setState({radioState: {state: "Error"}});
        render(<PageTabs />);

        fireEvent.click(screen.getByRole("button", {name: "Radio"}));

        expect(invoke).toHaveBeenCalledWith("radio_reconnect", undefined);
    });

    it("switches to the phone page", () => {
        useNavigationStore.getState().setPage("split");
        render(<PageTabs />);

        fireEvent.click(screen.getByRole("button", {name: "Phone"}));

        expect(useNavigationStore.getState().page).toBe("phone");
    });

    it("shows the radio tab active while on the radio page", () => {
        useNavigationStore.setState({page: "radio"});
        render(<PageTabs />);

        expect(screen.getByRole("button", {name: "Radio"})).toHaveClasses("active-tab");
    });

    it("switches pages and closes the settings menu when the menu is open", () => {
        useNavigationStore.setState({page: "split"});
        useNavigationStore.getState().openMenu("settings");
        render(<PageTabs />);

        fireEvent.click(screen.getByRole("button", {name: "Phone"}));

        expect(useNavigationStore.getState().menu).toBeUndefined();
        expect(useNavigationStore.getState().page).toBe("phone");
    });
});
