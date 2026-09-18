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

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import PageCycleButton from "../../../src/components/ui/PageCycleButton.tsx";
import {useNavigationStore} from "../../../src/stores/navigation-store.ts";
import {useRadioStore} from "../../../src/stores/radio-store.ts";

function getButton() {
    return screen.getByRole("button");
}

function activeCells() {
    return Array.from(getButton().querySelectorAll("p")).filter(p =>
        p.classList.contains("bg-gray-400"),
    );
}

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

describe("PageCycleButton", () => {
    it("cycles from radio to phone", () => {
        useNavigationStore.setState({page: "radio"});
        render(<PageCycleButton />);

        fireEvent.click(getButton());

        expect(useNavigationStore.getState().page).toBe("phone");
    });

    it("cycles from phone to split", () => {
        useNavigationStore.setState({page: "phone"});
        render(<PageCycleButton />);

        fireEvent.click(getButton());

        expect(useNavigationStore.getState().page).toBe("split");
    });

    it("cycles from split to radio", () => {
        useNavigationStore.setState({page: "split"});
        render(<PageCycleButton />);

        fireEvent.click(getButton());

        expect(useNavigationStore.getState().page).toBe("radio");
    });

    it("marks the current page's cell active", () => {
        useNavigationStore.setState({page: "split"});
        render(<PageCycleButton />);

        const active = activeCells();
        expect(active).toHaveLength(1);
        expect(active[0].textContent).toBe("M");
    });

    it("reconnects the radio when moving to radio and it is disconnected", () => {
        useNavigationStore.setState({page: "split"});
        useRadioStore.setState({radioState: {state: "Disconnected"}});
        render(<PageCycleButton />);

        fireEvent.click(getButton());

        expect(useNavigationStore.getState().page).toBe("radio");
        expect(invoke).toHaveBeenCalledWith("radio_reconnect", undefined);
    });

    it("reconnects the radio when moving to split and it is in an error state", () => {
        useNavigationStore.setState({page: "phone"});
        useRadioStore.setState({radioState: {state: "Error"}});
        render(<PageCycleButton />);

        fireEvent.click(getButton());

        expect(useNavigationStore.getState().page).toBe("split");
        expect(invoke).toHaveBeenCalledWith("radio_reconnect", undefined);
    });

    it("does not reconnect when moving to phone", () => {
        useNavigationStore.setState({page: "radio"});
        useRadioStore.setState({radioState: {state: "Disconnected"}});
        render(<PageCycleButton />);

        fireEvent.click(getButton());

        expect(useNavigationStore.getState().page).toBe("phone");
        expect(invoke.mock.calls.map(call => call[0])).not.toContain("radio_reconnect");
    });

    it("does not reconnect an already connected radio when moving to radio", () => {
        useNavigationStore.setState({page: "split"});
        useRadioStore.setState({radioState: {state: "Connected"}});
        render(<PageCycleButton />);

        fireEvent.click(getButton());

        expect(invoke.mock.calls.map(call => call[0])).not.toContain("radio_reconnect");
    });

    it("closes the settings menu when cycling pages", () => {
        useNavigationStore.setState({page: "phone"});
        useNavigationStore.getState().openMenu("settings");
        render(<PageCycleButton />);

        fireEvent.click(getButton());

        expect(useNavigationStore.getState().menu).toBeUndefined();
    });
});
