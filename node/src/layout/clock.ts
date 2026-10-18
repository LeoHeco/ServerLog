/** How a record's time is written. */

const START = performance.now();
const pad = (n: number, w = 2): string => String(n).padStart(w, "0");

/** Milliseconds since serlog was loaded. */
export const elapsed = (): number => Math.floor(performance.now() - START);

function offset(t: Date, colon: boolean): string {
  const m = -t.getTimezoneOffset();
  const a = Math.abs(m);
  return `${m < 0 ? "-" : "+"}${pad(Math.floor(a / 60))}${colon ? ":" : ""}${pad(a % 60)}`;
}

function isoWeek(t: Date): readonly [number, number] {
  const d = new Date(Date.UTC(t.getFullYear(), t.getMonth(), t.getDate()));
  const day = d.getUTCDay() || 7;
  d.setUTCDate(d.getUTCDate() + 4 - day);
  const start = Date.UTC(d.getUTCFullYear(), 0, 1);
  return [d.getUTCFullYear(), Math.ceil(((d.getTime() - start) / 864e5 + 1) / 7)];
}

/**
 * strftime in local time for `%Y %y %m %d %H %M %S %j %F %T %z %:z %G %V %%`, plus `%.3f`, `%.6f` and `%.9f`
 * for fractions of a second; other directives pass through.
 */
export function strftime(t: Date, fmt: string): string {
  return fmt.replace(/%(:z|\.\d+f|.)/g, (all, d: string) => {
    switch (d) {
      case "Y":
        return String(t.getFullYear());
      case "y":
        return pad(t.getFullYear() % 100);
      case "m":
        return pad(t.getMonth() + 1);
      case "d":
        return pad(t.getDate());
      case "H":
        return pad(t.getHours());
      case "M":
        return pad(t.getMinutes());
      case "S":
        return pad(t.getSeconds());
      case "j":
        return pad(Math.floor((t.getTime() - new Date(t.getFullYear(), 0, 1).getTime()) / 864e5) + 1, 3);
      case "F":
        return strftime(t, "%Y-%m-%d");
      case "T":
        return strftime(t, "%H:%M:%S");
      case "z":
        return offset(t, false);
      case ":z":
        return offset(t, true);
      case "G":
        return String(isoWeek(t)[0]);
      case "V":
        return pad(isoWeek(t)[1]);
      case "%":
        return "%";
      default:
        return d.startsWith(".") ? `.${`${pad(t.getMilliseconds(), 3)}000000`.slice(0, Number(d.slice(1, -1)))}` : all;
    }
  });
}

/** UTC RFC 3339 with `digits` decimals: `2023-11-14T22:13:20.000Z`. */
export function iso(t: Date, digits = 3): string {
  const base = t.toISOString().slice(0, 19);
  return digits ? `${base}.${`${pad(t.getUTCMilliseconds(), 3)}000000`.slice(0, digits)}Z` : `${base}Z`;
}

/** `t` in the format a `{time}` token asks for. */
export function writeTime(spec: string, t: Date): string {
  switch (spec) {
    case "":
    case "local":
      return strftime(t, "%Y-%m-%d %H:%M:%S%.3f");
    case "utc":
      return iso(t, 0);
    case "rfc3339":
      return strftime(t, "%Y-%m-%dT%H:%M:%S%.6f%:z");
    case "unix":
      return String(Math.floor(t.getTime() / 1000));
    case "unix_ms":
      return String(t.getTime());
    case "unix_ns":
      return String(BigInt(t.getTime()) * 1_000_000n);
    default:
      return strftime(t, spec);
  }
}
