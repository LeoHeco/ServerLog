"""The alarm route: one queue and one task, both made when the first incident arrives."""

import os
import queue
import sys
import threading
from typing import TYPE_CHECKING

from .. import guard

if TYPE_CHECKING:
    from . import Alarm, Incident

__lock = threading.Lock()
__tasks: list[queue.Queue] = []
__dropped = [0]


def __start() -> queue.Queue:
    q: queue.Queue = queue.Queue(64)

    def run() -> None:
        from .. import hub
        from . import Fallback, sound

        guard.adopt()
        while True:
            msg = q.get()
            match msg:
                case ("job", alarm, incident):
                    if sound(alarm, incident, hub.BOARD) == "unheard" and alarm.fallback == Fallback.PANIC:
                        crash(f"no sink took the alarm for `{incident.sink}`: {incident.error}")
                case ("settle", done):
                    done.set()
            q.task_done()

    threading.Thread(target=run, name="serlog-alarm", daemon=True).start()
    return q


def __queue() -> queue.Queue:
    with __lock:
        if not __tasks:
            __tasks.append(__start())
        return __tasks[0]


def post(alarm: Alarm, incident: Incident) -> None:
    """Queues `incident` for `alarm`; never blocks, and counts what a full queue drops."""
    try:
        __queue().put_nowait(("job", alarm, incident))
    except queue.Full:
        __dropped[0] += 1


def settle(timeout: float) -> None:
    """Waits up to `timeout` seconds for the incidents queued so far."""
    if not __tasks:
        return
    done = threading.Event()
    try:
        __tasks[0].put_nowait(("settle", done))
    except queue.Full:
        return
    done.wait(timeout)


def dropped() -> int:
    """Incidents a full alarm queue dropped."""
    return __dropped[0]


def forget() -> None:
    """Drops the task after a fork, where its thread no longer exists."""
    __tasks.clear()


def crash(why: str) -> None:
    """Says why on standard error, gives the other sinks a second to flush, and exits with 70."""
    from .. import hub

    try:
        sys.stderr.write(f"serlog: {why}; exiting\n")
        sys.stderr.flush()
    except OSError:
        pass
    hub.flush_slots(1.0)
    os._exit(70)
