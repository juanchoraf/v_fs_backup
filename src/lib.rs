use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::error::Error as StdError;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc::{self, Sender},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use v_concat::{v_concat, v_concat_eprintln, v_concat_println};

mod updater;

include!("runtime.rs");
include!("windows_terminal.rs");
include!("core.rs");

include!("types.rs");
include!("completion.rs");
include!("archive_types.rs");
include!("cli.rs");
include!("app.rs");
include!("interactive.rs");
include!("backup.rs");
include!("restore.rs");
include!("selectors.rs");
include!("streams.rs");
include!("legacy_rle.rs");
include!("progress.rs");
include!("walk.rs");
include!("sha256.rs");
include!("metadata.rs");
include!("archive_codec.rs");
include!("restore_paths.rs");
include!("path_match.rs");
include!("output.rs");

#[cfg(test)]
mod tests;
