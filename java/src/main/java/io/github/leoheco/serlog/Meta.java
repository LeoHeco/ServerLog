package io.github.leoheco.serlog;

import java.net.InetAddress;
import java.net.UnknownHostException;

/// Who is logging, attached to records that leave the process.
///
/// @param app the application; the main class or jar by default
/// @param env the environment, empty by default
/// @param instance this instance; the host name by default
public record Meta(String app, String env, String instance) {
    private static final class Host {
        static final String NAME = find();

        static String find() {
            for (var v : new String[] {System.getenv("HOSTNAME"), System.getenv("COMPUTERNAME")})
                if (v != null && !v.isBlank()) return v;
            try {
                return InetAddress.getLocalHost().getHostName();
            } catch (UnknownHostException e) {
                return "localhost";
            }
        }
    }

    /// The host name, as far as the environment tells.
    public static String host() {
        return Host.NAME;
    }

    /// The program name and the host name.
    public static Meta detect() {
        var cmd = System.getProperty("sun.java.command", "").strip();
        var first = cmd.isEmpty() ? "" : cmd.split("\\s+")[0];
        var base = first.substring(Math.max(first.lastIndexOf('/'), first.lastIndexOf('\\')) + 1);
        var app = base.endsWith(".jar")
                ? base.substring(0, base.length() - 4)
                : base.substring(base.lastIndexOf('.') + 1);
        return new Meta(app, "", host());
    }
}
