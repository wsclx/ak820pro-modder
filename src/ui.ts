import type { CSSProperties } from "react";

export const mono = "ui-monospace, Menlo, monospace";

export const card: CSSProperties = {
  marginBottom: 24,
  padding: 24,
  borderRadius: 8,
  border: "1px solid #262d3a",
  background: "#11141b",
};

export const sectionTitle: CSSProperties = {
  fontSize: 18,
  fontWeight: 500,
  margin: "0 0 16px",
};

export const subtle: CSSProperties = {
  color: "#7e8aa3",
  margin: 0,
};

export const label: CSSProperties = {
  color: "#7e8aa3",
};

export function hex4(n: number) {
  return "0x" + n.toString(16).padStart(4, "0");
}

export const primaryButton: CSSProperties = {
  background: "#7c5cff",
  color: "white",
  border: "none",
  borderRadius: 6,
  padding: "8px 16px",
  fontSize: 14,
  fontWeight: 500,
  cursor: "pointer",
};

export const ghostButton: CSSProperties = {
  background: "transparent",
  color: "#cbd3e1",
  border: "1px solid #262d3a",
  borderRadius: 6,
  padding: "6px 12px",
  fontSize: 13,
  cursor: "pointer",
};

export const ghostButtonActive: CSSProperties = {
  ...ghostButton,
  borderColor: "#7c5cff",
  background: "rgba(124, 92, 255, 0.12)",
  color: "white",
};
