package io.github.leoheco.serlog;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import tools.jackson.databind.json.JsonMapper;

/// The shared vectors in `spec/cases`.
final class Vectors {
    static final Path SPEC = Path.of("..", "spec");

    private Vectors() {}

    @SuppressWarnings("unchecked")
    static Map<String, Object> load(String name) throws IOException {
        return JsonMapper.builder()
                .build()
                .readValue(Files.readString(SPEC.resolve("cases").resolve(name + ".json")), Map.class);
    }
}
