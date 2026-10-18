package io.github.leoheco.serlog.config;

import java.nio.file.Path;
import java.util.Locale;

/// Config file syntax.
public enum Format {
    YAML,
    JSON,
    TOML;

    /// The format a file's extension names; YAML for anything but `.json` and `.toml`.
    public static Format of(Path path) {
        var name = path.getFileName().toString().toLowerCase(Locale.ROOT);
        return name.endsWith(".json") ? JSON : name.endsWith(".toml") ? TOML : YAML;
    }

    /// A format by its name.
    public static Format parse(String name) {
        return valueOf(name.strip().toUpperCase(Locale.ROOT));
    }
}
