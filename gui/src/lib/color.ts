export interface Rgb {
  r: number;
  g: number;
  b: number;
}

export const rgbToHex = ({ r, g, b }: Rgb): string =>
  [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");

/** Parses a 6-digit hex string (no leading #). Returns null if invalid. */
export function hexToRgb(hex: string): Rgb | null {
  if (!/^[0-9a-fA-F]{6}$/.test(hex)) return null;
  return {
    r: parseInt(hex.slice(0, 2), 16),
    g: parseInt(hex.slice(2, 4), 16),
    b: parseInt(hex.slice(4, 6), 16),
  };
}

const lum = (c: number) => {
  const f = c / 255;
  return f <= 0.03928 ? f / 12.92 : Math.pow((f + 0.055) / 1.055, 2.4);
};

/** Relative luminance (0..1) for a colour. */
export const luminance = ({ r, g, b }: Rgb): number =>
  0.2126 * lum(r) + 0.7152 * lum(g) + 0.0722 * lum(b);

/** Readable text colour on top of the given colour. */
export const textOn = (c: Rgb): string =>
  luminance(c) > 0.42 ? "#0b0d10" : "#f2f4f7";

export const cssRgb = ({ r, g, b }: Rgb): string => `rgb(${r} ${g} ${b})`;
