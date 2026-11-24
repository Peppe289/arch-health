//! procfs/sysfs/syscall adapters: filesystems, btrfs counters, kernel and
//! processes using replaced libraries.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use arch_health_core::ports::{
    BtrfsDevice, Drive, Filesystem, StaleLibraries, Storage, SystemInfo,
};
use arch_health_core::{PortError, PortResult};

use crate::dbus;

const REAL_FS: &[&str] = &[
    "ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "vfat", "exfat", "ntfs3", "zfs", "bcachefs",
];
/// Filesystems without a fixed inode table.
const NO_INODE_TABLE: &[&str] = &["btrfs", "vfat", "exfat", "zfs", "bcachefs"];

// ------------------------------------------------------------------ storage

pub struct LinuxStorage;

/// Decodes the octal escapes (`\040` …) used in /proc/self/mounts.
pub fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && i + 3 < b.len()
            && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c))
        {
            out.push((b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0'));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Storage for LinuxStorage {
    fn filesystems(&self) -> PortResult<Vec<Filesystem>> {
        let text = fs::read_to_string("/proc/self/mounts")
            .map_err(|e| PortError::unavailable(format!("reading /proc/self/mounts: {e}")))?;
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for line in text.lines() {
            let mut f = line.split_whitespace();
            let (Some(dev), Some(target), Some(fstype), Some(opts)) =
                (f.next(), f.next(), f.next(), f.next())
            else {
                continue;
            };
            // btrfs subvolumes share the device: report it once.
            if !REAL_FS.contains(&fstype) || !seen.insert(dev.to_string()) {
                continue;
            }
            let target = unescape_mount(target);
            let Ok(st) = rustix::fs::statvfs(target.as_str()) else {
                continue;
            };
            let has_inodes = st.f_files > 0 && !NO_INODE_TABLE.contains(&fstype);
            out.push(Filesystem {
                mount_point: target,
                device: unescape_mount(dev),
                fstype: fstype.to_string(),
                read_only: opts.split(',').any(|o| o == "ro"),
                bytes_used: st.f_blocks.saturating_sub(st.f_bfree) * st.f_frsize,
                bytes_available: st.f_bavail * st.f_frsize,
                inodes_used: has_inodes.then(|| st.f_files.saturating_sub(st.f_ffree)),
                inodes_available: has_inodes.then_some(st.f_favail),
            });
        }
        Ok(out)
    }

    fn btrfs_devices(&self) -> PortResult<Vec<BtrfsDevice>> {
        let Ok(filesystems) = fs::read_dir("/sys/fs/btrfs") else {
            return Ok(vec![]);
        };
        let mut out = Vec::new();
        for fs_dir in filesystems.flatten() {
            for dev in fs::read_dir(fs_dir.path().join("devinfo"))
                .into_iter()
                .flatten()
                .flatten()
            {
                let Ok(stats) = fs::read_to_string(dev.path().join("error_stats")) else {
                    continue;
                };
                let counters = stats
                    .lines()
                    .filter_map(|l| l.split_once(' '))
                    .filter_map(|(k, v)| Some((k.to_string(), v.trim().parse().ok()?)))
                    .collect();
                out.push(BtrfsDevice {
                    fs_uuid: fs_dir.file_name().to_string_lossy().into_owned(),
                    device_id: dev.file_name().to_string_lossy().into_owned(),
                    counters,
                });
            }
        }
        Ok(out)
    }

    fn drives(&self) -> PortResult<Vec<Drive>> {
        dbus::udisks_drives()
    }
}

// ------------------------------------------------------------------- system

pub struct LinuxSystem;

/// A mapped shared library that was deleted/replaced on disk:
/// `... /usr/lib/libssl.so.3 (deleted)`.
pub fn stale_library_path(maps_line: &str) -> Option<&str> {
    let path = maps_line.strip_suffix(" (deleted)")?.rsplit(' ').next()?;
    let lib = path.starts_with("/usr/") || path.starts_with("/lib");
    (lib && path.contains(".so")).then_some(path)
}

impl SystemInfo for LinuxSystem {
    fn is_root(&self) -> bool {
        rustix::process::geteuid().is_root()
    }

    fn running_kernel(&self) -> String {
        rustix::system::uname()
            .release()
            .to_string_lossy()
            .into_owned()
    }

    /// Only directories with a kernel image: after an upgrade, leftovers
    /// (e.g. DKMS modules) may keep the old directory without `vmlinuz`.
    fn installed_kernels(&self) -> Vec<String> {
        let mut out: Vec<String> = fs::read_dir("/usr/lib/modules")
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join("vmlinuz").is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        out.sort();
        out
    }

    fn stale_libraries(&self) -> PortResult<StaleLibraries> {
        let mut processes: BTreeMap<String, usize> = BTreeMap::new();
        let mut complete = true;
        let entries = fs::read_dir("/proc")
            .map_err(|e| PortError::unavailable(format!("reading /proc: {e}")))?;
        for entry in entries.flatten() {
            if !entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
            {
                continue;
            }
            let maps = match fs::read_to_string(entry.path().join("maps")) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    complete = false;
                    continue;
                }
                // The process exited meanwhile.
                Err(_) => continue,
            };
            // Empty for kernel threads and for other users' processes.
            if maps.is_empty() {
                continue;
            }
            if maps.lines().any(|l| stale_library_path(l).is_some()) {
                let comm = fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
                *processes.entry(comm.trim().to_string()).or_default() += 1;
            }
        }
        // Without root, other users' maps are unreadable or empty.
        complete &= self.is_root();
        Ok(StaleLibraries {
            processes,
            complete,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_mount_paths() {
        assert_eq!(unescape_mount("/mnt/my\\040disk"), "/mnt/my disk");
        assert_eq!(unescape_mount("/plain"), "/plain");
    }

    #[test]
    fn detects_deleted_libs() {
        let l = "7f00-7f01 r-xp 00000000 00:1f 123 /usr/lib/libssl.so.3 (deleted)";
        assert_eq!(stale_library_path(l), Some("/usr/lib/libssl.so.3"));
        assert_eq!(
            stale_library_path("7f00-7f01 rw-s 0 00:01 9 /memfd:foo (deleted)"),
            None
        );
        assert_eq!(
            stale_library_path("7f00-7f01 r-xp 0 00:1f 1 /usr/lib/libc.so.6"),
            None
        );
    }
}
