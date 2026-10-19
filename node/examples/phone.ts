// A sink of your own, from definition to use: texting warnings and errors to a phone.

import * as serlog from "serlog";

// #region stubs
function fooPhoneIsOk(phone: number | string): boolean {
  return /^\d+$/.test(String(phone));
}

function fooUsePhoneToLog(phone: number | string, text: string): void {
  console.log(`sms ${phone}: ${text}`);
}
// #endregion

// #region sink
class Phone implements serlog.Sink {
  readonly kind = "phone";
  readonly slot: serlog.SlotOptions;
  readonly phone: number | string;

  constructor({ phone, ...slot }: { phone?: number | string } & serlog.SlotOptions) {
    if (phone !== undefined && fooPhoneIsOk(phone)) this.phone = phone;
    else throw new serlog.ConfigError(`cannot text ${phone}`);
    this.slot = slot;
  }

  emit(entry: serlog.Entry): void {
    fooUsePhoneToLog(this.phone, entry.line);
  }
}
// #endregion

// #region register
serlog.kind("phone", (o) => new Phone(o));
// #endregion

// #region refuse
try {
  new Phone({ phone: "call me maybe" });
} catch (e) {
  console.log(`refused: ${(e as Error).message}`);
}
// #endregion

// #region config
await serlog.loads(`
app: mirror
sinks:
  - kind: console
    pattern: "{level:<5} {msg}{fields}"
  - kind: phone
    name: oncall
    phone: 13800138000
    level: warn
    pattern: "{level} {msg}"
`);
// #endregion

// #region use
const log = serlog.get("sync", { mirror: "debian" });
log.info({ files: 42 }, "synced");
log.warn({ free: "9%" }, "disk nearly full");
log.error({ tries: 3 }, "sync failed");
await serlog.flush();
// #endregion

// #region status
for (const s of serlog.status()) console.log(`${s.name}: ${s.kind}, ${s.health}`);
// #endregion
