import {defineConfig} from "oxlint";

export default defineConfig({
    plugins: ["typescript", "react", "react-perf", "vitest"],
    categories: {
        suspicious: "warn",
    },
    ignorePatterns: ["dist"],
    rules: {
        "react/react-in-jsx-scope": "off",
        "react/jsx-key": [
            "error",
            {
                checkFragmentShorthand: true,
            },
        ],

        "no-duplicate-imports": "error",
        "no-else-return": "warn",
        "no-empty": [
            "warn",
            {
                allowEmptyCatch: true,
            },
        ],
        "no-lonely-if": "error",
        "no-var": "warn",
        "prefer-template": "warn",

        "no-unsafe-type-assertion": "off",
        "no-unnecessary-type-arguments": "off",
        "no-shadow": "off",
    },
    options: {
        typeAware: true,
    },
    env: {
        builtin: true,
        browser: true,
    },
});
