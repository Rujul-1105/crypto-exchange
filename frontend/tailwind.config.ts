import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./src/**/*.{js,ts,jsx,tsx,mdx}"],
  theme: {
    extend: {
      colors: {
        // CEX-ish dark palette
        bg: { base: "#0b0d12", surface: "#11141b", raised: "#171b24" },
        line: { DEFAULT: "#222633", strong: "#2e3445" },
        text: { primary: "#e6e8ee", muted: "#9aa3b2", dim: "#6b7280" },
        accent: { buy: "#16a34a", sell: "#dc2626", neutral: "#3b82f6" },
      },
      fontFamily: {
        mono: ["ui-monospace", "SFMono-Regular", "Menlo", "monospace"],
      },
    },
  },
  plugins: [],
};
export default config;
