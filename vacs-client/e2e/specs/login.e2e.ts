import {authenticate, resetMockState} from "../helpers/auth.ts";
import {click} from "../helpers/browser.ts";

describe("Login Flow", () => {
    beforeEach(async () => {
        await resetMockState();
        await browser.reloadSession();
    });

    it("should show login page on startup", async () => {
        const loginButton = await $("button=Login via VATSIM");
        await loginButton.waitForDisplayed({timeout: 15_000});
    });

    it("should authenticate via mock OAuth and show connect page", async () => {
        const loginButton = await $("button=Login via VATSIM");
        await loginButton.waitForDisplayed({timeout: 15_000});

        await authenticate(browser, "10000001");

        const connectButton = await $("button=Connect");
        await connectButton.waitForDisplayed({timeout: 10_000});
    });

    it("should connect to signaling server after authentication", async () => {
        const loginButton = await $("button=Login via VATSIM");
        await loginButton.waitForDisplayed({timeout: 15_000});

        await authenticate(browser, "10000001");

        const connectButton = await $("button=Connect");
        await connectButton.waitForDisplayed({timeout: 10_000});

        await click(connectButton);

        await connectButton.waitForDisplayed({timeout: 15_000, reverse: true});
    });
});
