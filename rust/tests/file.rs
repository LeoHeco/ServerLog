//! The file sink: rolling by size and time, clean-up, compression, names, reopening and buffering.

#![allow(clippy::unwrap_used, reason = "test helpers fail loudly")]

use std::{
    fs,
    io::Read,
    path::Path,
    time::{Duration, SystemTime},
};

use serlog::{
    Entry, Layout, Level, Record, Sink,
    sinks::{File, Rotate},
};

fn put(f: &File, msg: &str, time: SystemTime) {
    let layout = Layout::pattern("{msg}").unwrap();
    f.emit(&Entry::new(
        Record::new(Level::INFO, "t", format_args!("{msg}")).stamped(time),
        &layout,
    ))
    .unwrap();
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn read(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    if path.extension().is_some_and(|e| e == "gz") {
        let mut s = String::new();
        flate2::read::GzDecoder::new(&bytes[..])
            .read_to_string(&mut s)
            .unwrap();
        s
    } else {
        String::from_utf8(bytes).unwrap()
    }
}

#[test]
fn rolls_by_size_keeps_the_newest_and_gzips() {
    let dir = tempfile::tempdir().unwrap();
    let f = File::new(dir.path().join("app.log"))
        .max_size(150)
        .keep(2)
        .gzip();
    f.ready().unwrap();
    let start = SystemTime::now();
    for i in 0..12 {
        put(
            &f,
            &format!("line {i:02} {}", "x".repeat(60)),
            start + Duration::from_secs(i),
        );
        f.flush().unwrap();
    }
    let all = names(dir.path());
    let rolled: Vec<&String> = all.iter().filter(|n| n.ends_with(".log.gz")).collect();
    assert_eq!(rolled.len(), 2, "{all:?}");
    assert_eq!(all.len(), 3, "{all:?}");
    assert!(read(&dir.path().join("app.log")).contains("line 11"));
    let text: String = rolled.iter().map(|n| read(&dir.path().join(n))).collect();
    assert!(
        text.contains("line 07") && text.contains("line 09") && !text.contains("line 05"),
        "{text}"
    );
}

#[test]
fn rotates_when_the_period_turns() {
    let dir = tempfile::tempdir().unwrap();
    let f = File::new(dir.path().join("day.log")).rotate(Rotate::Day);
    f.ready().unwrap();
    let now = SystemTime::now();
    put(&f, "today", now);
    put(&f, "still today", now);
    put(&f, "tomorrow", now + Duration::from_hours(25));
    f.flush().unwrap();
    let old: Vec<String> = names(dir.path())
        .into_iter()
        .filter(|n| n != "day.log")
        .collect();
    assert_eq!(old.len(), 1, "{old:?}");
    assert!(read(&dir.path().join(&old[0])).contains("still today"));
    assert_eq!(read(&dir.path().join("day.log")), "tomorrow\n");
}

#[test]
fn names_rolled_files_from_a_template() {
    let dir = tempfile::tempdir().unwrap();
    let f = File::new(dir.path().join("svc.log"))
        .max_size(10)
        .roll_name("{stem}-old.{ext}");
    f.ready().unwrap();
    let now = SystemTime::now();
    put(&f, "first line", now);
    put(&f, "second line", now);
    put(&f, "third line", now);
    f.flush().unwrap();
    assert_eq!(
        names(dir.path()),
        ["svc-old-1.log", "svc-old.log", "svc.log"]
    );
}

#[test]
fn cleans_up_by_total_size() {
    let dir = tempfile::tempdir().unwrap();
    let f = File::new(dir.path().join("big.log"))
        .max_size(100)
        .max_total(250);
    f.ready().unwrap();
    let now = SystemTime::now();
    for i in 0..8 {
        put(&f, &format!("{i} {}", "y".repeat(97)), now);
        f.flush().unwrap();
    }
    let rolled: u64 = names(dir.path())
        .iter()
        .filter(|n| *n != "big.log")
        .map(|n| fs::metadata(dir.path().join(n)).unwrap().len())
        .sum();
    assert!(rolled <= 250, "{rolled}");
}

#[test]
fn leaves_unrelated_files_alone() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("app.yaml"), "x").unwrap();
    let f = File::new(dir.path().join("app.log")).max_size(10).keep(0);
    f.ready().unwrap();
    put(&f, "first line", SystemTime::now());
    put(&f, "second line", SystemTime::now());
    f.flush().unwrap();
    assert_eq!(names(dir.path()), ["app.log", "app.yaml"]);
}

#[test]
fn reopens_after_the_file_is_moved_away() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("moved.log");
    let f = File::new(&path).reopen();
    f.ready().unwrap();
    put(&f, "before", SystemTime::now());
    f.flush().unwrap();
    fs::rename(&path, dir.path().join("moved.log.1")).unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    put(&f, "after", SystemTime::now());
    f.flush().unwrap();
    assert_eq!(read(&path), "after\n");
}

#[test]
fn truncates_when_asked_and_creates_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deep/er/t.log");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "old\n").unwrap();
    let f = File::new(&path).truncate();
    f.ready().unwrap();
    put(&f, "new", SystemTime::now());
    f.flush().unwrap();
    assert_eq!(read(&path), "new\n");
    let fresh = File::new(dir.path().join("a/b/c.log"));
    fresh.ready().unwrap();
    assert!(dir.path().join("a/b/c.log").exists());
    assert!(
        File::new(dir.path().join("x/y.log"))
            .no_mkdir()
            .ready()
            .is_err()
    );
}

#[test]
fn buffers_until_flushed_or_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("buf.log");
    let f = File::new(&path).buffer(1 << 16, Duration::from_hours(1));
    f.ready().unwrap();
    put(&f, "first", SystemTime::now());
    put(&f, "second", SystemTime::now());
    assert_eq!(read(&path), "first\n");
    let layout = Layout::pattern("{msg}").unwrap();
    f.emit(&Entry::new(
        Record::new(Level::ERROR, "t", format_args!("bad")),
        &layout,
    ))
    .unwrap();
    assert_eq!(read(&path), "first\nsecond\nbad\n");
}
