/**
 * A modern, high-performance logging framework: any number of sinks, each configured on its own.
 *
 * ```ts
 * import * as serlog from "serlog";
 *
 * serlog.info({ mirror: "debian" }, "synced %d files", 42);
 * await serlog.init({ sinks: [new serlog.Console(), new serlog.File({ path: "logs/app.log", rotate: "day", keep: 7 })] });
 * ```
 *
 * With no set-up at all it prints `info` and above to standard output, like most loggers.
 */

import "./sinks/console.ts";

export {
  Alarm,
  DEFAULT as DEFAULT_ALARM,
  dropped as alarmsDropped,
  type Failure,
  type Fallback,
  type Incident,
  setAlarm,
  Tiers,
  type TiersOptions,
} from "./alarm/index.ts";
export { capture, debugDirectives } from "./bridge.ts";
export { type Colour, Theme, type When } from "./colour.ts";
export { type Compat, compat, type Stream } from "./compat.ts";
export * as config from "./config/index.ts";
export { alarm, dump, type Format, kind, layout, load, loads, processor, watch } from "./config/index.ts";
export { Claimed, ConfigError, KindError, NameTaken, NotReady, Rejected, SerlogError, Twice } from "./errors.ts";
export { Filter } from "./filter.ts";
export {
  add,
  emit,
  enabled,
  flush,
  getFilter,
  process,
  redact,
  remove,
  setFilter,
  setLevel,
  setProcess,
  status,
} from "./hub.ts";
export { Json, type JsonOptions, type Layout, type Levels, Pattern, type Schema } from "./layout/index.ts";
export { defineLevel, Level, type Levelish, levelName } from "./level.ts";
export {
  type Args,
  type CatchOptions,
  catching,
  catchUncaught,
  debug,
  error,
  exception,
  fatal,
  get,
  info,
  Lazy,
  Logger,
  lazy,
  log,
  trace,
  warn,
} from "./logger.ts";
export { type Meta, meta } from "./meta.ts";
export {
  Dedupe,
  Exclude,
  Fields,
  Limit,
  Match,
  type Processor,
  Redact,
  Sample,
  Sanitise,
  type Step,
} from "./process/index.ts";
export { type Full, type Queue, queue } from "./queue.ts";
export { type Fields as FieldList, field, type LogRecord, record } from "./record.ts";
export { bind } from "./scope.ts";
export { Entry, type Sink, type SlotOptions } from "./sink.ts";
export {
  Console,
  type ConsoleOptions,
  File,
  type FileOptions,
  Loki,
  type LokiOptions,
  Mail,
  type MailOptions,
  Memory,
  type MemoryOptions,
  type Rotate,
} from "./sinks/index.ts";
export { type Health, Slot, type Status } from "./slot/index.ts";
export { init, isStarted, type Settings } from "./start.ts";
