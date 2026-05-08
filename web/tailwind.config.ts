import type { Config } from "tailwindcss";

export default {
  content: ["./app/**/*.{ts,tsx}", "./components/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // DeepSeek-aligned palette: cool white + soft gray, indigo accents.
        // (Previous warm cream `#F4F1E8` read too "Anthropic-like".)
        paper: "#FFFFFF",
        "paper-deep": "#F4F6FB",
        "paper-edge": "#E5E8F0",
        "paper-line": "#0E0E10",
        "paper-line-soft": "#D4D8E2",
        ink: "#0E0E10",
        "ink-soft": "#2E2E33",
        "ink-mute": "#6B7280",
        indigo: "#4D6BFE",
        "indigo-deep": "#3A52CC",
        "indigo-pale": "#E9EEFE",
        ochre: "#9C7A3F",
        jade: "#0AB68B",
        cobalt: "#1F3A8A",
      },
      fontFamily: {
        display: ['"Fraunces"', '"Noto Serif SC"', "ui-serif", "Georgia", "serif"],
        body: ['"IBM Plex Sans"', '"Noto Sans SC"', "ui-sans-serif", "system-ui", "sans-serif"],
        cjk: ['"Noto Serif SC"', '"Source Han Serif SC"', "serif"],
        mono: ['"JetBrains Mono"', "ui-monospace", "Menlo", "monospace"],
      },
      letterSpacing: {
        crisp: "-0.018em",
        wider: "0.08em",
        widest: "0.18em",
      },
    },
  },
  plugins: [],
} satisfies Config;
