"""Email for serious records; a batch goes out as one message, plain text and HTML."""

import html
import smtplib
import ssl
from collections.abc import Sequence
from email.message import EmailMessage
from email.utils import parseaddr
from importlib.resources import files
from typing import override

from ..compat import compat
from ..errors import ConfigError, Rejected
from ..level import Level
from ..meta import meta
from ..queues import Queue
from ..sink import Entry, Sink
from ..units import seconds, show


class Mail(Sink):
    """Sends batches by SMTP; only `error` and above unless the slot sets a level or filter.

    The config keys `pass` and `from` are Python keywords; in code they may also be written `password` and `sender`.
    """

    kind = "mail"
    __PAGE = files("serlog").joinpath("mail.html").read_text(encoding="utf-8")

    def __init__(
        self,
        host: str,
        *,
        to: str | Sequence[str] = (),
        port: int | None = None,
        security: str = "tls",
        user: str | None = None,
        password: str | None = None,
        sender: str | None = None,
        timeout: float | str = 10,
        connect: bool = False,
        **rest: object,
    ) -> None:
        password = rest.pop("pass", password)  # type: ignore[assignment]
        sender = rest.pop("from", sender)  # type: ignore[assignment]
        super().__init__(**rest)  # type: ignore[arg-type]
        if security not in ("tls", "starttls", "none"):
            raise ConfigError(f"`security` takes tls, starttls or none, not {security!r}")
        self.__host, self.__security, self.__port = host, security, port
        self.__to = [to] if isinstance(to, str) else list(to)
        self.__user, self.__password, self.__sender = user, password, sender
        self.__timeout, self.__connect = seconds(timeout), connect
        self.__min = Level.TRACE if "level" in rest or "filter" in rest else Level.ERROR

    def __from(self) -> str:
        return self.__sender or self.__user or ""

    @override
    def enabled(self, level: int, target: str) -> bool:
        return level >= self.__min

    @override
    def ready(self) -> None:
        if not self.__to:
            raise ConfigError("mail: no recipients")
        if not self.__from():
            raise ConfigError("mail: no sender")
        for a in (self.__from(), *self.__to):
            if "@" not in parseaddr(a)[1]:
                raise ConfigError(f"mail: bad address {a!r}")
        if self.__user and not self.__password:
            raise ConfigError("mail: a user needs a password")
        if self.__connect:
            with self.__open():
                pass

    def __open(self) -> smtplib.SMTP:
        port = self.__port or {"tls": 465, "starttls": 587, "none": 25}[self.__security]
        if self.__security == "tls":
            smtp: smtplib.SMTP = smtplib.SMTP_SSL(self.__host, port, context=ssl.create_default_context(), timeout=self.__timeout)
        else:
            smtp = smtplib.SMTP(self.__host, port, timeout=self.__timeout)
            if self.__security == "starttls":
                smtp.starttls(context=ssl.create_default_context())
        if self.__user and self.__password:
            smtp.login(self.__user, self.__password)
        return smtp

    def render(self, entries: Sequence[Entry]) -> EmailMessage:
        """The message for one batch, plain text with an HTML alternative."""
        m = meta()
        top = max(e.record.level for e in entries)
        n = len(entries)
        msg = EmailMessage()
        msg["Subject"] = f"[{m.app}] {n} {top.label(compat())} record{'' if n == 1 else 's'}"
        msg["From"], msg["To"] = self.__from(), ", ".join(self.__to)
        msg.set_content("".join(e.line + "\n" for e in entries))
        rows = "".join(
            f'<tr><td>{e.record.time.astimezone():%Y-%m-%d %H:%M:%S}</td><td class="{e.record.level.name}">{e.record.level.label(compat())}</td>'
            f"<td>{html.escape(e.record.target)}</td><td>{html.escape(e.record.msg)}</td></tr>"
            for e in entries
        )
        page = self.__PAGE
        for k, v in (
            ("app", html.escape(m.app)),
            ("env", html.escape(m.env)),
            ("instance", html.escape(m.instance)),
            ("level", top.name),
            ("count", str(n)),
            ("rows", rows),
        ):
            page = page.replace("{{" + k + "}}", v)
        msg.add_alternative(page, subtype="html")
        return msg

    @override
    def emit(self, entry: Entry) -> None:
        self.emit_batch([entry])

    @override
    def emit_batch(self, entries: Sequence[Entry]) -> None:
        if not entries:
            return
        try:
            with self.__open() as smtp:
                smtp.send_message(self.render(entries))
        except smtplib.SMTPResponseException as e:
            if e.smtp_code >= 500:
                raise Rejected(f"mail refused: {e.smtp_code} {e.smtp_error!r}") from e
            raise

    @override
    def queued(self) -> Queue:
        return Queue(1024, wait=60)

    @override
    def options(self) -> dict[str, object]:
        out: dict[str, object] = {"host": self.__host}
        if self.__port:
            out["port"] = self.__port
        if self.__security != "tls":
            out["security"] = self.__security
        if self.__user:
            out["user"] = self.__user
            out["pass"] = "***"
        if self.__sender:
            out["from"] = self.__sender
        out["to"] = list(self.__to)
        if self.__timeout != 10:
            out["timeout"] = show(self.__timeout)
        if self.__connect:
            out["connect"] = True
        return out
