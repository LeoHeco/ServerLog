package io.github.leoheco.serlog.slf4j;

import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ConcurrentMap;
import org.slf4j.ILoggerFactory;
import org.slf4j.IMarkerFactory;
import org.slf4j.Logger;
import org.slf4j.helpers.BasicMDCAdapter;
import org.slf4j.helpers.BasicMarkerFactory;
import org.slf4j.spi.MDCAdapter;
import org.slf4j.spi.SLF4JServiceProvider;

/// serlog as an SLF4J 2 provider: with serlog on the class path, every SLF4J logger writes through it. JUL and the
/// Log4j API come in through SLF4J's own bridges, `jul-to-slf4j` and `log4j-to-slf4j`.
public final class SerlogProvider implements SLF4JServiceProvider {
    private final ConcurrentMap<String, Logger> loggers = new ConcurrentHashMap<>();
    private final ILoggerFactory factory = name -> loggers.computeIfAbsent(name, SerlogLogger::new);
    private final IMarkerFactory markers = new BasicMarkerFactory();
    private final MDCAdapter mdc = new BasicMDCAdapter();

    @Override
    public ILoggerFactory getLoggerFactory() {
        return factory;
    }

    @Override
    public IMarkerFactory getMarkerFactory() {
        return markers;
    }

    @Override
    public MDCAdapter getMDCAdapter() {
        return mdc;
    }

    @Override
    public String getRequestedApiVersion() {
        return "2.0.99";
    }

    @Override
    public void initialize() {}
}
