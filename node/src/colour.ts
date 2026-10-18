/** Terminal colours, per level and overridable, honouring `NO_COLOR` and `FORCE_COLOR`. */

/** A terminal colour. */
export type Colour = "plain" | "red" | "green" | "yellow" | "blue" | "magenta" | "cyan" | "white" | "grey" | "alarm";

const ANSI: Readonly<Record<Colour, string>> = {
  plain: "",
  red: "\x1b[1;31m",
  green: "\x1b[32m",
  yellow: "\x1b[33m",
  blue: "\x1b[34m",
  magenta: "\x1b[35m",
  cyan: "\x1b[36m",
  white: "\x1b[37m",
  grey: "\x1b[90m",
  alarm: "\x1b[1;37;41m",
};

/** A colour by name; `gray` means `grey`. */
export function parseColour(name: string): Colour {
  const n = name === "gray" ? "grey" : name;
  if (!(n in ANSI)) throw new TypeError(`unknown colour \`${name}\``);
  return n as Colour;
}

/** When to colour. */
export type When = "auto" | "always" | "never";

/** Decides for a sink that is, or is not, a terminal. */
export function resolveWhen(when: When, tty: boolean): boolean {
  if (when !== "auto") return when === "always";
  if (process.env.FORCE_COLOR) return true;
  return tty && !process.env.NO_COLOR;
}

/** `text` in `colour` when `on`. */
export const paint = (on: boolean, colour: Colour, text: string): string =>
  on && colour !== "plain" ? `${ANSI[colour]}${text}\x1b[0m` : text;

/** Colours by level; levels not listed keep their own. */
export class Theme {
  readonly #over: ReadonlyMap<number, Colour>;

  constructor(over: Iterable<readonly [number, Colour]> = []) {
    this.#over = new Map(over);
  }

  /** The colour for `level`, given its own. */
  of(level: number, own: Colour): Colour {
    return this.#over.get(level) ?? own;
  }

  /** The overrides. */
  get overrides(): ReadonlyMap<number, Colour> {
    return this.#over;
  }
}
