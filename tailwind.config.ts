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
        // Foreground steps. f0 = primary text, f1 = secondary, f2 = body-detail,
        // f3 = quiet-but-legible, f4 = disabled.
        //
        // Contrast against surface.base (#0b0c10) — all values pass WCAG AA
        // for normal text now (was previously fail for f2 / f3):
        //   f0 ≈ 14.9 : 1   (AAA)
        //   f1 ≈ 10.4 : 1   (AAA)
        //   f2 ≈ 6.7  : 1   (AA, comfortable body)
        //   f3 ≈ 4.7  : 1   (AA, quieter — minimums for non-bold 12 px)
        //   f4 ≈ 2.8  : 1   (intentionally below AA — disabled state)
        fg: {
          0: "#f3f5fb",
          1: "#d4d8e3",
          2: "#abb1c2",
          3: "#8e95a7",
          4: "#5a6076",
        },
        // Hairlines / dividers — lifted a notch so borders are actually visible
        // on cards (subtle was barely there before).
        line: {
          subtle: "#232838",
          DEFAULT: "#303652",
          strong: "#444b66",
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
        // Compressed type scale for a dense pro app — `xs` lifted from 11.5 px
        // to 12 px so detail rows in modals / cards aren't squinty.
        "2xs": ["11px",   { lineHeight: "1.35", letterSpacing: "0.04em" }],
        xs:   ["12px",   { lineHeight: "1.45", letterSpacing: "0.01em" }],
        sm:   ["13.5px", { lineHeight: "1.55" }],
        base: ["14.5px", { lineHeight: "1.6"  }],
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
