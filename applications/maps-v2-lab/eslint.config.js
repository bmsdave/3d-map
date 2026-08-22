import ts from "@typescript-eslint/parser";
export default [
  {
    files: ["src/**/*.ts"],
    languageOptions: { parser: ts, ecmaVersion: 2022, sourceType: "module" },
    rules: {
      "no-console": ["warn", { allow: ["warn", "error"] }],
      "@typescript-eslint/no-unused-vars": ["warn", { argsIgnorePattern: "^_" }],
    },
  },
  { ignores: ["dist/**", "src/generated/**", "coverage/**", "node_modules/**"] },
];
