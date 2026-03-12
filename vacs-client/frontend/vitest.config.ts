import {defineConfig} from "vitest/config";
import preact from "@preact/preset-vite";

export default defineConfig({
    plugins: [preact()],
    test: {
        environment: "jsdom",
        setupFiles: ["./test/setup.ts"],
        include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
        globals: true,
    },
});
