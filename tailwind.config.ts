import type { Config } from "tailwindcss";

/**
 * Design tokens for the AK820 Pro control app.
 *
 * Colors are anchored on OKLCH and translated to hex here so Tailwind v3 picks
 * them up cleanly. The greys are perceptually-uniform with a slight warm tint
 * (chroma > 0 toward 280°) — softer than cold blue-greys, more sophisticated
 * than pure neutral. The accent walks four OKLCH lightness stops so we can
 * express UI states (rest / hover / pressed / soft fill) without saturation
 * cliffs.
 */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Surface elevation 0–4. base = window background, surface = sidebar,
        // elevated = cards, overlay = popovers/modals, raised = active states.
        surface: {
          base:     "#0b0c10",  // L≈12 — deepest, never pure black
          surface:  "#11131a",  // L≈14 — sidebar
          elevated: "#171a23",  // L≈18 — cards
          raised:   "#1d212c",  // L≈22 — hover/active card
          overlay:  "#252a36",  // L≈26 — popovers
        },
        // Foreground steps. f0 = primary text, f1 = secondary, f2 = tertiary,
        // f3 = disabled/quiet.
        fg: {
          0: "#eef0f6",
          1: "#c1c6d3",
          2: "#8a91a3",
          3: "#5b6378",
          4: "#3c4254",
        },
        // Hairlines / dividers.
        line: {
          subtle: "#1c2030",
          DEFAULT: "#262b3c",
          strong: "#363c50",
        },
        // Accent (lilac → indigo). Each step ≈ +6 L in OKLCH.
        accent: {
          50:  "#f2efff",
          100: "#e3dcff",
          200: "#c8bbff",
          300: "#a98fff",
          400: "#8a6fff",
          500: "#7c5cff",   // primary
          600: "#6644ee",
          700: "#5234c4",
          800: "#3f2898",
          900: "#2a1c66",
          glow: "rgba(124, 92, 255, 0.16)",
          ring: "rgba(124, 92, 255, 0.42)",
        },
        good: { DEFAULT: "#3dd589", soft: "rgba(61, 213, 137, 0.15)" },
        warn: { DEFAULT: "#f5b342", soft: "rgba(245, 179, 66, 0.15)" },
        bad:  { DEFAULT: "#f56565", soft: "rgba(245, 101, 101, 0.15)" },
      },
      fontFamily: {
        sans: ["-apple-system", "BlinkMacSystemFont", '"SF Pro Text"', '"Inter"', "system-ui", "sans-serif"],
        mono: ["ui-monospace", "SFMono-Regular", '"JetBrains Mono"', "Menlo", "monospace"],
      },
      fontSize: {
        // Compressed type scale for a dense pro app.
        "2xs": ["10.5px", { lineHeight: "1.3", letterSpacing: "0.04em" }],
        xs:   ["11.5px", { lineHeight: "1.35", letterSpacing: "0.02em" }],
        sm:   ["13px",   { lineHeight: "1.5" }],
        base: ["14px",   { lineHeight: "1.55" }],
        lg:   ["16px",   { lineHeight: "1.5",  letterSpacing: "-0.005em" }],
        xl:   ["19px",   { lineHeight: "1.4",  letterSpacing: "-0.012em" }],
        "2xl":["24px",   { lineHeight: "1.25", letterSpacing: "-0.018em" }],
        "3xl":["30px",   { lineHeight: "1.15", letterSpacing: "-0.022em" }],
      },
      letterSpacing: {
        kicker: "0.08em",
      },
      borderRadius: {
        sm: "6px",
        DEFAULT: "8px",
        md: "10px",
        lg: "14px",
        xl: "18px",
      },
      boxShadow: {
        // Cards: rim-light inside + soft drop. Subtle is the point.
        card: "inset 0 1px 0 0 rgba(255,255,255,0.025), 0 1px 0 0 rgba(0,0,0,0.4), 0 12px 32px -16px rgba(0,0,0,0.55)",
        raised: "inset 0 1px 0 0 rgba(255,255,255,0.04), 0 2px 0 0 rgba(0,0,0,0.45), 0 16px 40px -16px rgba(0,0,0,0.65)",
        glow: "0 0 0 1px rgba(124,92,255,0.45), 0 0 24px -4px rgba(124,92,255,0.45)",
        press: "inset 0 1px 1px rgba(0,0,0,0.25)",
      },
      transitionTimingFunction: {
        spring: "cubic-bezier(0.34, 1.56, 0.64, 1)",
      },
    },
  },
  plugins: [],
} satisfies Config;
