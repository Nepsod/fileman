use nptk::std::ffi::OsString;
use nptk::std::collections::HashSet;
use nptk::std::fs;
use nptk::std::os::unix::ffi::OsStringExt;
use nptk::std::path::{Path, PathBuf};

use npio::mount::Mount;
use npio::service::volumemonitor::VolumeMonitor;
use npio::volume::Volume;

#[derive(Debug, Clone)]
pub struct VolumeMount {
    pub label: String,
    pub mount_point: PathBuf,
}

pub fn list_removable_mounts() -> Vec<VolumeMount> {
    let Ok(content) = fs::read_to_string("/proc/mounts") else {
        return Vec::new();
    };

    let mut mounts = Vec::new();
    for line in content.lines() {
        let Some((mount_point, remainder)) = parse_mount_line(line) else {
            continue;
        };
        if !is_removable_mount_path(&mount_point) {
            continue;
        }
        let label = mount_label(&mount_point, remainder);
        if mounts
            .iter()
            .any(|existing: &VolumeMount| existing.mount_point == mount_point)
        {
            continue;
        }
        mounts.push(VolumeMount {
            label,
            mount_point,
        });
    }

    mounts.sort_by(|left, right| left.label.cmp(&right.label));
    mounts
}

pub async fn collect_sidebar_mounts(monitor: &VolumeMonitor) -> Vec<VolumeMount> {
    let mut mounts = Vec::new();
    let mut seen_paths = HashSet::new();

    for volume in monitor.get_volumes().await {
        let label = volume.get_name();
        let Some(mount) = volume.get_mount() else {
            continue;
        };
        let Some(mount_point) = mount_path_from_mount(mount.as_ref()) else {
            continue;
        };
        if !is_sidebar_mount_path(&mount_point) || !seen_paths.insert(mount_point.clone()) {
            continue;
        }
        mounts.push(VolumeMount {
            label,
            mount_point,
        });
    }

    for mount in monitor.get_mounts().await {
        let Some(mount_point) = mount_path_from_mount(mount.as_ref()) else {
            continue;
        };
        if !seen_paths.insert(mount_point.clone()) || !is_sidebar_mount_path(&mount_point) {
            continue;
        }
        let label = mount.get_name();
        mounts.push(VolumeMount { label, mount_point });
    }

    mounts.sort_by(|left, right| left.label.cmp(&right.label));
    mounts
}

pub async fn run_volume_monitor_loop(
    mut on_update: impl FnMut(Vec<VolumeMount>) + Send,
) {
    let monitor = VolumeMonitor::new();
    if monitor.start(None).await.is_err() {
        on_update(list_removable_mounts());
        return;
    }

    on_update(collect_mounts_for_sidebar(&monitor).await);

    let mut event_receiver = monitor.subscribe();
    while event_receiver.recv().await.is_ok() {
        on_update(collect_mounts_for_sidebar(&monitor).await);
    }
}

async fn collect_mounts_for_sidebar(monitor: &VolumeMonitor) -> Vec<VolumeMount> {
    let mounts = collect_sidebar_mounts(monitor).await;
    if mounts.is_empty() {
        list_removable_mounts()
    } else {
        mounts
    }
}

fn mount_path_from_mount(mount: &dyn Mount) -> Option<PathBuf> {
    let uri = mount.get_root().uri();
    uri_to_path(&uri)
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let path_string = uri.strip_prefix("file://").unwrap_or(uri);
    let path_string = path_string
        .strip_prefix("localhost")
        .unwrap_or(path_string);
    if path_string.is_empty() {
        return None;
    }
    Some(PathBuf::from(percent_decode_os(path_string)))
}

fn percent_decode_os(encoded: &str) -> OsString {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""), 16)
            {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    OsString::from_vec(decoded)
}

fn unescape_proc_mount_path(mount_point: &str) -> String {
    mount_point
        .replace("\\\\", "\\")
        .replace("\\040", " ")
        .replace("\\011", "\t")
}

fn is_removable_mount_path(mount_point: &Path) -> bool {
    let path_string = mount_point.to_string_lossy();
    path_string.starts_with("/media/")
        || path_string.starts_with("/run/media/")
        || path_string.starts_with("/mnt/")
}

fn is_sidebar_mount_path(mount_point: &Path) -> bool {
    if is_removable_mount_path(mount_point) {
        return true;
    }

    let path_string = mount_point.to_string_lossy();
    if path_string.starts_with("/run/user/") {
        let parts: Vec<&str> = path_string.split('/').filter(|segment| !segment.is_empty()).collect();
        return parts.len() > 3;
    }

    !matches!(
        path_string.as_ref(),
        "/"
            | "/dev"
            | "/sys"
            | "/proc"
            | "/tmp"
            | "/boot"
            | "/var"
            | "/usr"
            | "/etc"
            | "/bin"
            | "/sbin"
            | "/lib"
            | "/opt"
            | "/snap"
            | "/run/credentials"
            | "/run"
            | "/home"
            | "/cache"
            | "/root"
    ) && !path_string.starts_with("/dev/")
        && !path_string.starts_with("/sys/")
        && !path_string.starts_with("/proc/")
        && !path_string.starts_with("/tmp/")
        && !path_string.starts_with("/boot/")
        && !path_string.starts_with("/var/")
        && !path_string.starts_with("/usr/")
        && !path_string.starts_with("/etc/")
        && !path_string.starts_with("/bin/")
        && !path_string.starts_with("/sbin/")
        && !path_string.starts_with("/lib/")
        && !path_string.starts_with("/opt/")
        && !path_string.starts_with("/snap/")
        && !path_string.starts_with("/run/credentials/")
}

fn parse_mount_line(line: &str) -> Option<(PathBuf, &str)> {
    let mut fields = line.split_whitespace();
    let _device = fields.next()?;
    let mount_point = fields.next()?;
    let filesystem = fields.next()?;
    Some((
        PathBuf::from(unescape_proc_mount_path(mount_point)),
        filesystem,
    ))
}

fn mount_label(mount_point: &Path, filesystem: &str) -> String {
    mount_point
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} ({})", mount_point.display(), filesystem))
}
