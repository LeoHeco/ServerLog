"""A registered slot: the core both the caller and a background task use, and the queue in front of it."""

import queue
import threading
import time
from collections.abc import Callable
from contextlib import suppress
from datetime import UTC, datetime
from typing import Literal

from .. import alarms, guard
from ..colour import Theme
from ..errors import SerlogError, transient
from ..layouts import Layout
from ..processors import Step, run
from ..queues import Full, Queue
from ..record import Record
from ..sink import Entry, Sink
from . import Slot, Status
from .breaker import Breaker, Health, Phase, Tell

type Policy = Literal["panic", "ignore"] | alarms.Alarm


def policy(on_fail: object, named: dict[str, alarms.Alarm]) -> Policy:
    """The failure policy an `on_fail` setting names."""
    match on_fail:
        case None | "default":
            return alarms.DEFAULT
        case "panic" | "ignore":
            return on_fail  # type: ignore[return-value]
        case alarms.Alarm():
            return on_fail
        case str() if on_fail in named:
            return named[on_fail]
    raise SerlogError(f"`on_fail` names {on_fail!r}, which is not in `alarms`")


class Core:
    """The sink, its look, its policy and its health."""

    def __init__(self, name: str, slot: Slot, on_fail: Policy, retries: int) -> None:
        self.name = name
        self.sink: Sink = slot.sink
        self.kind = slot.sink.kind
        self.layout: Layout = slot.layout or slot.sink.default_layout()
        self.theme: Theme | None = slot.theme if slot.colour.resolve(slot.sink.tty()) else None
        self.on_fail = on_fail
        self.retries = retries
        self.breaker = Breaker(slot.probe, time.monotonic())
        self.lock = threading.Lock()
        self.dropped = 0
        self.suppressed = 0

    @property
    def up(self) -> bool:
        """Whether the sink is working."""
        return self.breaker.health is Health.UP

    @property
    def tell(self) -> Tell:
        """How its failures are told."""
        return Tell.REPORT if isinstance(self.on_fail, alarms.Alarm) else Tell.SILENT

    def entry(self, rec: Record, cache: dict | None = None) -> Entry:
        """`rec` laid out the way this slot lays it out."""
        return Entry(rec, self.layout, self.theme, cache)

    def attempt(self, fn: Callable[[], None], pause: bool) -> None:
        """Calls `fn` up to the retry count, pausing 1, 2, 4… seconds between tries when `pause` is set."""
        with guard.inside(self):
            for n in range(self.retries + 1):
                try:
                    fn()
                    return
                except Exception as e:
                    if not transient(e) or n == self.retries:
                        raise
                    if pause:
                        time.sleep(2 ** min(n, 5))

    def write(self, entry: Entry) -> None:
        """Writes in the caller's thread."""
        if self.on_fail == "panic":
            try:
                self.attempt(lambda: self.sink.emit(entry), False)
            except Exception as e:
                raise SerlogError(f"sink `{self.name}` failed: {e}") from e
            return
        if not self.up:
            with self.lock:
                if not self.breaker.admit(time.monotonic()):
                    return
        try:
            self.attempt(lambda: self.sink.emit(entry), False)
        except Exception as e:
            self.failed(e, self.tell)
        else:
            self.passed()

    def drain(self, buf: list[Record], patient: bool = True) -> None:
        """Writes a batch from the queue, waiting out failures until it goes through or is refused."""
        while buf:
            if not self.up:
                if not patient:
                    break
                time.sleep(self.breaker.wait(time.monotonic()))
                with self.lock:
                    self.breaker.admit(time.monotonic())
            entries = [self.entry(r) for r in buf]
            try:
                self.attempt(lambda batch=entries: self.sink.emit_batch(batch), self.up)
            except Exception as e:
                if self.on_fail == "panic":
                    alarms.task.crash(f"sink `{self.name}` failed: {e}")
                self.failed(e, self.tell)
                if not transient(e):
                    self.dropped += len(buf)
                    buf.clear()
            else:
                self.passed()
                buf.clear()
        self.dropped += len(buf)
        buf.clear()

    def deliver(self, rec: Record) -> None:
        """Writes an alarm record straight to the sink; a failure rides in the incident being delivered."""
        if not self.up:
            with self.lock:
                if not self.breaker.admit(time.monotonic()):
                    raise SerlogError("down")
        try:
            with guard.inside(self):
                self.sink.emit_batch([self.entry(rec)])
                self.sink.flush()
        except Exception as e:
            self.failed(e, Tell.FOLDED)
            raise
        self.passed()

    def resting(self) -> bool:
        """Whether alarms leave it out now."""
        return not self.up and self.breaker.resting(time.monotonic())

    def passed(self) -> None:
        """A write went through."""
        if self.up:
            return
        with self.lock:
            phase = self.breaker.passed()
        if phase:
            self.notify(phase)

    def failed(self, e: BaseException, tell: Tell) -> None:
        """A write failed for good."""
        remind = self.on_fail.remind if isinstance(self.on_fail, alarms.Alarm) else float("inf")
        with self.lock:
            phase = self.breaker.failed(time.monotonic(), str(e) or type(e).__name__, remind, tell)
        if phase:
            self.notify(phase)

    def notify(self, phase: Phase) -> None:
        """Raises an incident for this phase."""
        if not isinstance(self.on_fail, alarms.Alarm):
            return
        b = self.breaker
        incident = alarms.Incident(self.name, self.kind, phase, b.error or "", skipped=b.skipped_now, dropped=self.dropped, since=b.since)
        alarms.post(self.on_fail, incident)

    def status(self) -> Status:
        """The slot's state."""
        b = self.breaker
        return Status(self.name, self.kind, b.health, b.skipped, self.dropped, self.suppressed, b.since, b.error)


class Lane:
    """A slot's queue and the thread draining it, both made when the first record arrives."""

    __STOP = object()

    def __init__(self, shape: Queue, core: Core) -> None:
        self.shape, self.core = shape, core
        self.__q: queue.Queue | None = None
        self.__thread: threading.Thread | None = None
        self.__lock = threading.Lock()

    def __queue(self) -> queue.Queue:
        with self.__lock:
            if self.__q is None:
                self.__q = queue.Queue(max(self.shape.size, 1))
                self.__thread = threading.Thread(target=self.__run, args=(self.__q,), name=f"serlog-{self.core.name}", daemon=True)
                self.__thread.start()
            return self.__q

    def push(self, rec: Record) -> None:
        """Queues `rec`, or counts it as dropped when the queue is full."""
        q = self.__queue()
        try:
            if self.shape.full is Full.BLOCK and self.core.up:
                q.put(rec)
            else:
                q.put_nowait(rec)
        except queue.Full:
            self.core.dropped += 1

    def flush(self, deadline: float) -> None:
        """Waits until what is queued has been written, unless the sink is down or the deadline passes."""
        q = self.__q
        if q is None or not self.core.up:
            return
        done = threading.Event()
        try:
            q.put(done, timeout=max(deadline - time.monotonic(), 0))
        except queue.Full:
            return
        done.wait(max(deadline - time.monotonic(), 0))

    def stop(self, within: float = 5) -> None:
        """Lets the thread write what it holds and end, waiting up to `within` seconds."""
        if self.__q is not None and self.__thread is not None:
            self.__q.put(self.__STOP)
            self.__thread.join(within)

    def forget(self) -> None:
        """Drops the thread after a fork, where it no longer exists."""
        self.__q = None
        self.__lock = threading.Lock()

    def __run(self, q: queue.Queue) -> None:
        guard.adopt(self.core)
        buf: list[Record] = []
        due = 0.0
        while True:
            try:
                msg = q.get(timeout=max(due - time.monotonic(), 0) if buf else None)
            except queue.Empty:
                self.core.drain(buf)
                continue
            if msg is self.__STOP:
                self.core.drain(buf, patient=False)
                self.__flush_sink()
                return
            if isinstance(msg, threading.Event):
                self.core.drain(buf)
                self.__flush_sink()
                msg.set()
                continue
            if not buf:
                due = time.monotonic() + self.shape.wait
            buf.append(msg)
            if len(buf) >= self.shape.batch:
                self.core.drain(buf)

    def __flush_sink(self) -> None:
        try:
            self.core.sink.flush()
        except Exception as e:
            self.core.failed(e, self.core.tell)


class Live:
    """A registered slot."""

    def __init__(self, slot: Slot, name: str, named: dict[str, alarms.Alarm]) -> None:
        shape = slot.sink.queued() if slot.mode is True else slot.mode
        retries = slot.retries if slot.retries is not None else 3 if shape else 0
        self.core = Core(name, slot, policy(slot.on_fail, named), retries)
        self.slot = slot
        self.filter = slot.filter
        self.process: tuple[Step, ...] = slot.process
        self.alarm_only = slot.alarm_only
        self.claim = slot.sink.claim()
        self.queue: Queue | None = shape  # type: ignore[assignment]
        self.lane = Lane(shape, self.core) if isinstance(shape, Queue) else None
        self.added = datetime.now(UTC)

    @property
    def name(self) -> str:
        """The slot's name."""
        return self.core.name

    def emit(self, rec: Record, cache: dict) -> None:
        """Takes one record from the hub."""
        if self.filter is not None and not self.filter.allows(rec.level, rec.target):
            return
        if not self.core.sink.enabled(rec.level, rec.target):
            return
        if self.process:
            with guard.inside():
                out = run(self.process, rec)
            if out is None:
                return
            if out is not rec:
                rec, cache = out, {}
        if self.lane is not None:
            self.lane.push(rec)
        else:
            self.core.write(self.core.entry(rec, cache))

    def retire(self) -> None:
        """Writes what the queue holds, stops its thread and closes the sink."""
        if self.lane is not None:
            self.lane.stop()
        elif self.core.up:
            self.flush(time.monotonic() + 5)
        with guard.inside(self.core), suppress(Exception):
            self.core.sink.close()

    def flush(self, deadline: float) -> None:
        """Waits for the queue, or flushes the sink."""
        if self.lane is not None:
            self.lane.flush(deadline)
        elif self.core.up:
            try:
                self.core.sink.flush()
            except Exception as e:
                self.core.failed(e, self.core.tell)
