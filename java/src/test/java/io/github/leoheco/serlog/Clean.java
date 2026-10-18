package io.github.leoheco.serlog;

import io.github.leoheco.serlog.sink.MemorySink;
import java.util.List;
import org.junit.jupiter.api.extension.AfterEachCallback;
import org.junit.jupiter.api.extension.BeforeEachCallback;
import org.junit.jupiter.api.extension.ExtensionContext;

/// Removes the slots a test added, and sets the level to `trace` for it.
final class Clean implements BeforeEachCallback, AfterEachCallback {
    private static final ExtensionContext.Namespace NS = ExtensionContext.Namespace.create(Clean.class);

    @Override
    public void beforeEach(ExtensionContext ctx) {
        Serlog.setLevel(Level.TRACE);
        if (Serlog.status().stream().noneMatch(s -> s.name().equals("main")))
            Serlog.add(new MemorySink(1).name("main").filter("off"));
        ctx.getStore(NS).put("before", names());
    }

    @Override
    @SuppressWarnings("unchecked")
    public void afterEach(ExtensionContext ctx) {
        var before = (List<String>) ctx.getStore(NS).get("before");
        for (var n : names()) if (!before.contains(n)) Serlog.remove(n);
    }

    static List<String> names() {
        return Serlog.status().stream().map(Status::name).toList();
    }
}
