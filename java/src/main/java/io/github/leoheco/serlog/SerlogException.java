package io.github.leoheco.serlog;

/// Anything serlog throws. The subclasses say what went wrong: at start-up, while registering a sink, or inside
/// one.
public sealed class SerlogException extends RuntimeException
        permits SerlogException.Config,
                SerlogException.NameTaken,
                SerlogException.Claimed,
                SerlogException.NotReady,
                SerlogException.Rejected,
                SerlogException.Twice {
    public SerlogException(String message) {
        super(message);
    }

    public SerlogException(String message, Throwable cause) {
        super(message, cause);
    }

    /// A setting is missing or malformed.
    public static sealed class Config extends SerlogException permits Kind {
        public Config(String message) {
            super(message);
        }

        public Config(String message, Throwable cause) {
            super(message, cause);
        }
    }

    /// A config names a kind nobody registered.
    public static final class Kind extends Config {
        public Kind(String message) {
            super(message);
        }
    }

    /// Two sinks share a name, or a name is reserved.
    public static final class NameTaken extends SerlogException {
        public NameTaken(String message) {
            super(message);
        }
    }

    /// A resource, such as a file, is already claimed by another sink.
    public static final class Claimed extends SerlogException {
        public Claimed(String message) {
            super(message);
        }
    }

    /// A sink failed its start-up check.
    public static final class NotReady extends SerlogException {
        private final String sink;

        public NotReady(String sink, Throwable cause) {
            super("sink `" + sink + "` is not ready: " + message(cause), cause);
            this.sink = sink;
        }

        /// The sink's name.
        public String sink() {
            return sink;
        }
    }

    /// The other end refused the records; sending them again will not help.
    public static final class Rejected extends SerlogException {
        public Rejected(String message) {
            super(message);
        }
    }

    /// `init` ran a second time.
    public static final class Twice extends SerlogException {
        public Twice() {
            super("serlog is already initialised");
        }
    }

    /// Whether trying again could succeed.
    public static boolean retryable(Throwable e) {
        return !(e instanceof Rejected || e instanceof Config);
    }

    /// A message for any throwable: its message, or its class's name when it has none.
    public static String message(Throwable e) {
        var m = e.getMessage();
        return m == null || m.isEmpty() ? e.getClass().getSimpleName() : m;
    }
}
