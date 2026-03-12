import "@testing-library/jest-dom/vitest";
import {mockIPC, clearMocks} from "@tauri-apps/api/mocks";
import {randomFillSync} from "crypto";
import {afterEach, beforeAll, beforeEach, vi} from "vitest";

// jsdom doesn't come with a WebCrypto implementation
beforeAll(() => {
    Object.defineProperty(window, "crypto", {
        value: {
            getRandomValues: (buffer: NodeJS.ArrayBufferView) => randomFillSync(buffer),
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
