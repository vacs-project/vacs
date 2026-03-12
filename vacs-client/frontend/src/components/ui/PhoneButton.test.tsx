import {describe, expect, it, afterEach} from "vitest";
import {render, screen, act} from "@testing-library/preact";
import PhoneButton from "./PhoneButton.tsx";
import {ButtonColor, ButtonColors, ButtonHighlightColor, ButtonHighlightColors} from "./Button.tsx";
import {useCallStore} from "../../stores/call-store.ts";
import {Call} from "../../types/call.ts";
import {CallId, ClientId, StationId} from "../../types/generic.ts";

const makeCall = (overrides: Partial<Call> = {}): Call => ({
    callId: "call0" as CallId,
    source: {clientId: "client0" as ClientId},
    target: {station: "station0" as StationId},
    prio: false,
    ...overrides,
});

afterEach(() => {
    useCallStore.getState().actions.reset();
});

function getButton() {
    return screen.getByRole("button", {name: "Phone"});
}

function expectButton(color: ButtonColor, highlight?: ButtonHighlightColor) {
    const btn = getButton();
    expect(btn).toHaveClass(ButtonColors[color]);
    if (highlight !== undefined) {
        expect(btn.querySelector("div")).toHaveClass(ButtonHighlightColors[highlight]);
    } else {
        expect(btn.querySelector("div")).toBeNull();
    }
}

describe("PhoneButton", () => {
    it("renders gray when idle", () => {
        render(<PhoneButton />);
        expectButton("gray");
    });

    describe("outgoing call", () => {
        it("renders gray with green highlight", () => {
            useCallStore.setState({
                callDisplay: {type: "outgoing", call: makeCall()},
            });
            render(<PhoneButton />);
            expectButton("gray", "green");
        });

        it("blinks between yellow with green highlight and gray with green highlight for priority call", () => {
            useCallStore.setState({
                callDisplay: {type: "outgoing", call: makeCall({prio: true})},
                blink: true,
            });
            render(<PhoneButton />);
            expectButton("yellow", "green");

            act(() => {
                useCallStore.setState({blink: false});
            });
            expectButton("gray", "green");

            act(() => {
                useCallStore.setState({blink: true});
            });
            expectButton("yellow", "green");
        });

        it("shows outgoing state when both outgoing and incoming calls exist", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "outgoing", call: makeCall()},
                incomingCalls: [makeCall({callId: "call1" as CallId})],
            });
            render(<PhoneButton />);
            expectButton("gray", "green");

            act(() => {
                useCallStore.setState({blink: false});
            });
            expectButton("gray", "green");

            act(() => {
                useCallStore.setState({blink: true});
            });
            expectButton("gray", "green");
        });

        it("ignores incoming prio when outgoing call exists", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "outgoing", call: makeCall()},
                incomingCalls: [makeCall({callId: "call1" as CallId, prio: true})],
            });
            render(<PhoneButton />);
            expectButton("gray", "green");

            act(() => {
                useCallStore.setState({blink: false});
            });
            expectButton("gray", "green");

            act(() => {
                useCallStore.setState({blink: true});
            });
            expectButton("gray", "green");
        });
    });

    describe("incoming call", () => {
        it("blinks between green and gray for incoming call", () => {
            useCallStore.setState({
                blink: true,
                incomingCalls: [makeCall()],
            });
            render(<PhoneButton />);
            expectButton("green");

            act(() => {
                useCallStore.setState({blink: false});
            });
            expectButton("gray");

            act(() => {
                useCallStore.setState({blink: true});
            });
            expectButton("green");
        });

        it("blinks between yellow with green highlight and gray for priority call", () => {
            useCallStore.setState({
                blink: true,
                incomingCalls: [makeCall({prio: true})],
            });
            render(<PhoneButton />);
            expectButton("yellow", "green");

            act(() => {
                useCallStore.setState({blink: false});
            });
            expectButton("gray", "gray");

            act(() => {
                useCallStore.setState({blink: true});
            });
            expectButton("yellow", "green");
        });
    });

    describe("accepted call", () => {
        it("renders green", () => {
            useCallStore.setState({
                callDisplay: {type: "accepted", call: makeCall()},
            });
            render(<PhoneButton />);
            expectButton("green");
        });

        it("renders yellow with green highlight for priority call", () => {
            useCallStore.setState({
                callDisplay: {type: "accepted", call: makeCall({prio: true})},
            });
            render(<PhoneButton />);
            expectButton("yellow", "green");
        });

        it("shows accepted state when incoming calls exist", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "accepted", call: makeCall()},
                incomingCalls: [makeCall({callId: "call1" as CallId})],
            });
            render(<PhoneButton />);
            expectButton("green");
        });
    });

    describe("rejected call", () => {
        it("blinks between green and gray with green highlight", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "rejected", call: makeCall()},
            });
            render(<PhoneButton />);
            expectButton("green", "green");

            act(() => {
                useCallStore.setState({blink: false});
            });
            expectButton("gray", "green");

            act(() => {
                useCallStore.setState({blink: true});
            });
            expectButton("green", "green");
        });
    });

    describe("error call", () => {
        it("blinks between red and gray", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "error", call: makeCall()},
            });
            render(<PhoneButton />);
            expectButton("red");

            act(() => {
                useCallStore.setState({blink: false});
            });
            expectButton("gray");

            act(() => {
                useCallStore.setState({blink: true});
            });
            expectButton("red");
        });
    });
});
