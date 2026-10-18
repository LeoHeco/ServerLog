package io.github.leoheco.serlog.config;

import io.github.leoheco.serlog.SerlogException;
import java.util.Map;

/// Reading and writing config text through Jackson, kept apart so the rest loads without it.
final class Codec {
    private Codec() {}

    private static String needs(Format f) {
        return switch (f) {
            case JSON -> "tools.jackson.core:jackson-databind";
            case YAML -> "tools.jackson.dataformat:jackson-dataformat-yaml";
            case TOML -> "tools.jackson.dataformat:jackson-dataformat-toml";
        };
    }

    private static tools.jackson.databind.ObjectMapper mapper(Format f) {
        return switch (f) {
            case JSON -> tools.jackson.databind.json.JsonMapper.builder().build();
            case YAML ->
                tools.jackson.dataformat.yaml.YAMLMapper.builder()
                        .disable(tools.jackson.dataformat.yaml.YAMLWriteFeature.WRITE_DOC_START_MARKER)
                        .enable(tools.jackson.dataformat.yaml.YAMLWriteFeature.MINIMIZE_QUOTES)
                        .build();
            case TOML -> tools.jackson.dataformat.toml.TomlMapper.builder().build();
        };
    }

    @SuppressWarnings("unchecked")
    static Map<String, Object> read(String text, Format f) {
        try {
            if (text.isBlank()) return Map.of();
            var tree = mapper(f).readValue(text, Object.class);
            if (tree == null) return Map.of();
            if (!(tree instanceof Map<?, ?> m)) throw new SerlogException.Config("the config must be a map");
            return (Map<String, Object>) m;
        } catch (NoClassDefFoundError e) {
            throw new SerlogException.Config("reading " + f.name() + " needs " + needs(f), e);
        } catch (tools.jackson.core.JacksonException e) {
            throw new SerlogException.Config("bad " + f.name() + ": " + e.getOriginalMessage(), e);
        }
    }

    static String write(Map<String, Object> tree, Format f) {
        try {
            var m = mapper(f);
            return f == Format.JSON
                    ? m.writerWithDefaultPrettyPrinter().writeValueAsString(tree) + "\n"
                    : m.writeValueAsString(tree);
        } catch (NoClassDefFoundError e) {
            throw new SerlogException.Config("writing " + f.name() + " needs " + needs(f), e);
        }
    }
}
