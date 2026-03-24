import {mockIPC, clearMocks} from "@tauri-apps/api/mocks";
import {afterEach, beforeAll, beforeEach, vi} from "vitest";
import "./matchers.ts";

// @ts-expect-error crypto is a nodejs built-in module
import {randomFillSync} from "crypto";

// jsdom doesn't come with a WebCrypto implementation
beforeAll(() => {
    Object.defineProperty(window, "crypto", {
        value: {
            // @ts-expect-error polyfilling required crypto functions for jsdom
            getRandomValues: buffer => randomFillSync(buffer),
        },
    });
});

// Set up Tauri IPC mock before each test.
// Individual tests can call mockIPC() again to override command responses.
beforeEach(() => {
    mockIPC(() => {}, {shouldMockEvents: true});
});

afterEach(() => {
    clearMocks();
});

// Mock Tauri plugins that aren't covered by mockIPC
vi.mock("@tauri-apps/plugin-log", () => ({
    error: vi.fn(),
    warn: vi.fn(),
    info: vi.fn(),
    debug: vi.fn(),
    trace: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
    openUrl: vi.fn(),
}));
