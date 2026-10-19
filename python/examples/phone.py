"""A sink of your own, from definition to use: texting warnings and errors to a phone."""

from typing import Unpack

import serlog


# region stubs
def foo_phone_is_ok(phone: int | str) -> bool:
    return str(phone).isdigit()


def foo_use_phone_to_log(phone: int | str, text: str) -> None:
    print(f"sms {phone}: {text}")


# endregion


# region sink
# region register
@serlog.kind("phone")
# endregion
class Phone(serlog.Sink):
    kind = "phone"
    phone: int | str

    def __init__(self, phone: int | str | None = None, **slot: Unpack[serlog.SlotKeys]) -> None:
        super().__init__(**slot)
        if phone is not None and foo_phone_is_ok(phone):
            self.phone = phone
        else:
            raise ValueError(f"cannot text {phone}")

    def emit(self, entry: serlog.Entry) -> None:
        foo_use_phone_to_log(self.phone, entry.line)


# endregion

# region refuse
try:
    Phone(phone="call me maybe")
except ValueError as e:
    print(f"refused: {e}")
# endregion

# region config
serlog.loads("""
app: mirror
sinks:
  - kind: console
    pattern: "{level:<5} {msg}{fields}"
  - kind: phone
    name: oncall
    phone: 13800138000
    level: warn
    pattern: "{level} {msg}"
""")
# endregion

# region use
log = serlog.get("sync", mirror="debian")
log.info("synced", files=42)
log.warn("disk nearly full", free="9%")
log.error("sync failed", tries=3)
serlog.flush()
# endregion

# region status
for s in serlog.status():
    print(f"{s.name}: {s.kind}, {s.health}")
# endregion
