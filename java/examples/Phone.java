// A sink of your own, from definition to use: texting warnings and errors to a phone.

import io.github.leoheco.serlog.Entry;
import io.github.leoheco.serlog.Serlog;
import io.github.leoheco.serlog.SinkBase;
import io.github.leoheco.serlog.config.Config;
import io.github.leoheco.serlog.config.Format;
import io.github.leoheco.serlog.config.Kinds;
import java.util.Map;

// region stubs
static boolean fooPhoneIsOk(String phone) {
    return phone.chars().allMatch(Character::isDigit);
}

static void fooUsePhoneToLog(String phone, String text) {
    IO.println("sms " + phone + ": " + text);
}
// endregion

// region sink
static final class Phone extends SinkBase<Phone> {
    private final String phone;

    Phone(long phone) {
        this(Long.toString(phone));
    }

    Phone(String phone) {
        if (phone != null && fooPhoneIsOk(phone)) this.phone = phone;
        else throw new IllegalArgumentException("cannot text " + phone);
    }

    @Override
    public String kind() {
        return "phone";
    }

    @Override
    public void emit(Entry entry) {
        fooUsePhoneToLog(phone, entry.line());
    }
}
// endregion

void main() {
    // region register
    Kinds.sink("phone", o -> new Phone(o.string("phone")));
    // endregion

    // region refuse
    try {
        new Phone("call me maybe");
    } catch (IllegalArgumentException e) {
        IO.println("refused: " + e.getMessage());
    }
    // endregion

    // region config
    Config.loads("""
            app: mirror
            sinks:
              - kind: console
                pattern: "{level:<5} {msg}{fields}"
              - kind: phone
                name: oncall
                phone: 13800138000
                level: warn
                pattern: "{level} {msg}"
            """, Format.YAML);
    // endregion

    // region use
    var log = Serlog.get("sync", Map.of("mirror", "debian"));
    log.atInfo().field("files", 42).log("synced");
    log.atWarn().field("free", "9%").log("disk nearly full");
    log.atError().field("tries", 3).log("sync failed");
    Serlog.flush();
    // endregion

    // region status
    for (var s : Serlog.status()) IO.println(s.name() + ": " + s.kind() + ", " + s.health().id());
    // endregion
}
