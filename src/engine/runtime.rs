use std::collections::HashSet;
use std::num::NonZeroU32;
use std::process::{Command, Output};

use anyhow::{anyhow, Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessId(NonZeroU32);

impl ProcessId {
    pub fn new(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProcessTree {
    pids: Vec<u32>,
}

impl ProcessTree {
    pub fn for_pid(pid: u32) -> Self {
        Self {
            pids: process_tree_pids(pid),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.pids.iter().copied()
    }

    pub fn env_var(&self, key: &str) -> Option<String> {
        self.find_map(|pid| process_environ_var(pid, key))
    }

    pub fn find_map<T>(&self, find: impl FnMut(u32) -> Option<T>) -> Option<T> {
        self.iter().find_map(find)
    }

    pub fn find_map_by_comm<T>(
        &self,
        name: &str,
        mut find: impl FnMut(u32) -> Option<T>,
    ) -> Option<T> {
        self.iter()
            .filter(|pid| process_comm(*pid).as_deref() == Some(name))
            .find_map(|pid| find(pid))
    }
}

pub fn socket_inode_from_fd_target(target: &str) -> Option<u64> {
    let value = target.trim().strip_prefix("socket:[")?.strip_suffix(']')?;
    value.parse::<u64>().ok().filter(|inode| *inode > 0)
}

pub fn socket_inodes_for_pid(pid: u32) -> HashSet<u64> {
    let mut inodes = HashSet::new();
    let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return inodes;
    };
    for entry in entries.flatten() {
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        let target = target.to_string_lossy();
        let Some(inode) = socket_inode_from_fd_target(&target) else {
            continue;
        };
        inodes.insert(inode);
    }
    inodes
}

pub fn socket_path_from_proc_net_unix(
    raw: &str,
    socket_inodes: &HashSet<u64>,
    path_contains: &str,
) -> Option<String> {
    if socket_inodes.is_empty() {
        return None;
    }
    for line in raw.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(inode) = fields.get(6).and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
        if !socket_inodes.contains(&inode) {
            continue;
        }
        let Some(path) = fields.get(7) else {
            continue;
        };
        if !path.contains(path_contains) {
            continue;
        }
        return Some((*path).to_string());
    }
    None
}

pub fn socket_path_for_pid_from_proc_net_unix(pid: u32, path_contains: &str) -> Option<String> {
    let socket_inodes = socket_inodes_for_pid(pid);
    let raw = std::fs::read_to_string("/proc/net/unix").ok()?;
    socket_path_from_proc_net_unix(&raw, &socket_inodes, path_contains)
}

pub fn socket_path_from_ss_output(
    raw: &str,
    socket_inodes: &HashSet<u64>,
    path_contains: &str,
) -> Option<String> {
    if socket_inodes.is_empty() {
        return None;
    }
    for line in raw.lines() {
        if !socket_inodes.iter().any(|inode| {
            line.contains(&format!(" {inode} "))
                || line.contains(&format!(" {inode} users:"))
                || line.ends_with(&format!(" {inode}"))
        }) {
            continue;
        }
        for token in line.split_whitespace() {
            if !token.starts_with('/') || !token.contains(path_contains) {
                continue;
            }
            return Some(token.to_string());
        }
    }
    None
}

pub fn socket_path_for_pid_from_ss(pid: u32, path_contains: &str) -> Option<String> {
    let socket_inodes = socket_inodes_for_pid(pid);
    if socket_inodes.is_empty() {
        return None;
    }
    let output = Command::new("ss").args(["-xnp"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    socket_path_from_ss_output(
        &String::from_utf8_lossy(&output.stdout),
        &socket_inodes,
        path_contains,
    )
}

pub fn all_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return pids;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Ok(pid) = name.to_string_lossy().parse::<u32>() else {
            continue;
        };
        pids.push(pid);
    }
    pids
}

#[derive(Debug, Clone)]
pub struct CommandContext {
    pub adapter: &'static str,
    pub action: &'static str,
    pub target: Option<String>,
}

impl CommandContext {
    pub fn new(adapter: &'static str, action: &'static str) -> Self {
        Self {
            adapter,
            action,
            target: None,
        }
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }
}

fn command_identity(program: &str, args: &[&str], context: &CommandContext) -> String {
    let rendered_args = if args.is_empty() {
        String::new()
    } else {
        format!(" {}", args.join(" "))
    };
    let target = context
        .target
        .as_deref()
        .map(|value| format!(" target={value}"))
        .unwrap_or_default();
    format!(
        "{}::{}{} => {}{}",
        context.adapter, context.action, target, program, rendered_args
    )
}

pub fn run_command_output(
    program: &str,
    args: &[&str],
    context: &CommandContext,
) -> Result<Output> {
    Command::new(program).args(args).output().with_context(|| {
        format!(
            "failed to execute {}",
            command_identity(program, args, context)
        )
    })
}

pub fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

pub fn run_command_status(program: &str, args: &[&str], context: &CommandContext) -> Result<()> {
    let output = run_command_output(program, args, context)?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = stderr_text(&output);
    let identity = command_identity(program, args, context);
    if stderr.is_empty() {
        Err(anyhow!("{} failed with status {}", identity, output.status))
    } else {
        Err(anyhow!("{} failed: {}", identity, stderr))
    }
}

/// Collect all direct child pids of all threads for a process.
pub fn child_pids(pid: u32) -> Vec<u32> {
    let task_dir = format!("/proc/{pid}/task");
    std::fs::read_dir(&task_dir)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|entry| {
            let tid = entry.file_name().to_string_lossy().parse::<u32>().ok();
            tid.map_or_else(Vec::new, |tid| {
                std::fs::read_to_string(format!("/proc/{pid}/task/{tid}/children"))
                    .map(|contents| {
                        contents
                            .split_whitespace()
                            .filter_map(|token| token.parse::<u32>().ok())
                            .collect::<Vec<u32>>()
                    })
                    .unwrap_or_default()
            })
        })
        .collect()
}

pub fn descendant_pids(pid: u32) -> Vec<u32> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = child_pids(pid);
    while let Some(current) = stack.pop() {
        if !seen.insert(current) {
            continue;
        }
        result.push(current);
        stack.extend(child_pids(current));
    }
    result
}

pub fn process_tree_pids(pid: u32) -> Vec<u32> {
    if pid == 0 {
        return Vec::new();
    }
    let mut result = descendant_pids(pid);
    result.insert(0, pid);
    result.sort_unstable();
    result.dedup();
    result
}

pub fn find_descendants_by_comm(pid: u32, name: &str) -> Vec<u32> {
    descendant_pids(pid)
        .into_iter()
        .filter(|candidate| process_comm(*candidate).as_deref() == Some(name))
        .collect()
}

pub fn normalize_process_name(comm: &str) -> String {
    let without_path = comm.rsplit('/').next().unwrap_or(comm).trim();
    without_path
        .split(':')
        .next()
        .unwrap_or(without_path)
        .trim()
        .to_string()
}

pub fn process_comm(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|value| normalize_process_name(value.trim()))
}

pub fn process_environ_var(pid: u32, key: &str) -> Option<String> {
    let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let prefix = format!("{key}=");
    for chunk in environ.split(|byte| *byte == 0) {
        let entry = String::from_utf8_lossy(chunk);
        if let Some(value) = entry.strip_prefix(&prefix) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

pub fn process_cmdline_args(pid: u32) -> Option<Vec<String>> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(
        cmdline
            .split(|byte| *byte == 0)
            .filter(|segment| !segment.is_empty())
            .map(|segment| String::from_utf8_lossy(segment).to_string())
            .collect(),
    )
}

pub fn process_fd_target(pid: u32, fd: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/fd/{fd}"))
        .ok()
        .map(|target| target.to_string_lossy().to_string())
}

pub fn process_uses_tty(pid: u32, tty_name: &str) -> bool {
    [0_u32, 1, 2]
        .into_iter()
        .filter_map(|fd| process_fd_target(pid, fd))
        .any(|target| target == tty_name)
}

pub fn is_shell_comm(comm: &str) -> bool {
    matches!(
        normalize_process_name(comm).as_str(),
        "bash" | "fish" | "zsh" | "sh" | "dash" | "ksh" | "tcsh" | "csh" | "nu" | "xonsh"
    )
}

pub fn is_shell_pid(pid: u32) -> bool {
    process_comm(pid)
        .map(|comm| is_shell_comm(&comm))
        .unwrap_or(false)
}

pub fn parse_stat_tpgid(stat: &str) -> Option<u32> {
    let after_comm = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    fields
        .get(5)?
        .parse::<i32>()
        .ok()
        .filter(|value| *value > 0)
        .map(|value| value as u32)
}

pub fn parse_stat_pgrp(stat: &str) -> Option<u32> {
    let after_comm = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    fields
        .get(2)?
        .parse::<i32>()
        .ok()
        .filter(|value| *value > 0)
        .map(|value| value as u32)
}

pub fn foreground_process_name_for_tty_in_tree(root_pid: u32, tty_name: &str) -> Option<String> {
    if root_pid == 0 || tty_name.trim().is_empty() {
        return None;
    }

    let candidates: Vec<(u32, String, Option<u32>, Option<u32>)> = process_tree_pids(root_pid)
        .into_iter()
        .filter(|pid| process_uses_tty(*pid, tty_name))
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok();
            let comm = process_comm(pid)?;
            Some((
                pid,
                comm,
                stat.as_deref().and_then(parse_stat_pgrp),
                stat.as_deref().and_then(parse_stat_tpgid),
            ))
        })
        .collect();

    let pick = |entries: &[(u32, String, Option<u32>, Option<u32>)]| {
        entries
            .iter()
            .rev()
            .find(|(_, comm, _, _)| !is_shell_comm(comm))
            .or_else(|| entries.iter().rev().next())
            .map(|(_, comm, _, _)| comm.clone())
    };

    let foreground_group: Vec<_> = candidates
        .iter()
        .filter(|(_, _, pgrp, tpgid)| pgrp.is_some() && pgrp == tpgid)
        .cloned()
        .collect();
    pick(&foreground_group).or_else(|| pick(&candidates))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::process::Output;

    use super::{
        foreground_process_name_for_tty_in_tree, parse_stat_pgrp, process_fd_target,
        process_tree_pids, process_uses_tty, socket_inode_from_fd_target,
        socket_path_from_proc_net_unix, socket_path_from_ss_output, stderr_text, stdout_text,
        CommandContext,
    };

    #[cfg(target_os = "linux")]
    use super::{
        all_pids, process_cmdline_args, process_comm, process_environ_var,
        socket_path_for_pid_from_proc_net_unix, ProcessTree,
    };

    #[cfg(target_os = "linux")]
    use std::os::unix::{
        net::{UnixListener, UnixStream},
        process::ExitStatusExt,
    };

    #[cfg(not(target_os = "linux"))]
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn command_context_builder_sets_target() {
        let context = CommandContext::new("tmux", "list-panes").with_target("%1");
        assert_eq!(context.adapter, "tmux");
        assert_eq!(context.action, "list-panes");
        assert_eq!(context.target.as_deref(), Some("%1"));
    }

    #[test]
    fn process_tree_pids_rejects_zero_pid() {
        assert!(process_tree_pids(0).is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_cmdline_args_reads_current_process() {
        let args = process_cmdline_args(std::process::id()).expect("current process has cmdline");
        assert!(!args.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_environ_var_reads_current_process() {
        if let Ok(path) = std::env::var("PATH") {
            assert_eq!(process_environ_var(std::process::id(), "PATH"), Some(path));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_tree_helper_reads_env_and_finds_current_process() {
        let tree = ProcessTree::for_pid(std::process::id());
        assert!(tree.iter().any(|pid| pid == std::process::id()));
        if let Ok(path) = std::env::var("PATH") {
            assert_eq!(tree.env_var("PATH"), Some(path));
        }
        assert!(tree
            .find_map(|pid| (pid == std::process::id()).then_some(pid))
            .is_some());
        if let Some(comm) = process_comm(std::process::id()) {
            assert!(tree
                .find_map_by_comm(&comm, |pid| (pid == std::process::id()).then_some(pid))
                .is_some());
        }
    }

    #[test]
    fn parse_stat_pgrp_reads_process_group() {
        let stat = "1234 (zsh) S 1 4321 4321 34817 4321 4194560 28954 275 0 0 32 5 0 0 20 0 1 0 123456 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
        assert_eq!(parse_stat_pgrp(stat), Some(4321));
        assert_eq!(super::parse_stat_tpgid(stat), Some(4321));
    }

    #[test]
    fn fd_target_helpers_handle_current_process() {
        let pid = std::process::id();
        let _ = process_fd_target(pid, 0);
        let _ = process_uses_tty(pid, "/dev/does-not-exist");
        let _ = foreground_process_name_for_tty_in_tree(pid, "/dev/does-not-exist");
    }

    #[test]
    fn extracts_socket_inode_from_fd_target() {
        assert_eq!(socket_inode_from_fd_target("socket:[458030]"), Some(458030));
        assert_eq!(socket_inode_from_fd_target("/dev/pts/1"), None);
    }

    #[test]
    fn extracts_socket_path_from_proc_net_unix_socket_entries() {
        let mut inodes = HashSet::new();
        inodes.insert(458030);
        let raw = r#"
Num       RefCount Protocol Flags    Type St Inode Path
0000000000000000: 00000002 00000000 00010000 0001 01 458030 /run/user/1000/zellij/0.43.1/implacable-oboe
0000000000000000: 00000003 00000000 00000000 0001 03 458031 /tmp/other.sock
"#;
        assert_eq!(
            socket_path_from_proc_net_unix(raw, &inodes, "zellij"),
            Some("/run/user/1000/zellij/0.43.1/implacable-oboe".to_string())
        );
    }

    #[test]
    fn extracts_socket_path_from_ss_output_via_peer_inode() {
        let mut inodes = HashSet::new();
        inodes.insert(455551);
        let raw = r#"
u_str ESTAB 0 0 * 455551 * 458031 users:(("zellij",pid=134518,fd=6),("zellij",pid=134518,fd=5))
u_str ESTAB 0 0 /run/user/1000/zellij/0.43.1/implacable-oboe 458031 * 455551 users:(("zellij",pid=134525,fd=7),("zellij",pid=134525,fd=6))
"#;
        assert_eq!(
            socket_path_from_ss_output(raw, &inodes, "zellij"),
            Some("/run/user/1000/zellij/0.43.1/implacable-oboe".to_string())
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn socket_path_for_pid_from_proc_net_unix_reads_proc_entries() {
        let base = std::env::temp_dir().join(format!(
            "yeet-and-yoink-runtime-socket-test-{}",
            std::process::id()
        ));
        let socket_dir = base.join("zellij");
        std::fs::create_dir_all(&socket_dir).expect("socket dir should be created");
        let socket_path = socket_dir.join("mock-session");
        let listener = UnixListener::bind(&socket_path).expect("unix listener should bind");
        let _stream = UnixStream::connect(&socket_path).expect("unix stream should connect");

        let discovered = socket_path_for_pid_from_proc_net_unix(std::process::id(), "zellij");
        drop(listener);
        let _ = std::fs::remove_file(&socket_path);
        let _ = std::fs::remove_dir_all(&base);

        assert_eq!(discovered, Some(socket_path.to_string_lossy().to_string()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn all_pids_includes_current_process() {
        assert!(all_pids().into_iter().any(|pid| pid == std::process::id()));
    }

    #[cfg(unix)]
    #[test]
    fn output_text_helpers_trim_output() {
        let output = Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: b" hello \n".to_vec(),
            stderr: b" error \n".to_vec(),
        };
        assert_eq!(stdout_text(&output), "hello");
        assert_eq!(stderr_text(&output), "error");
    }
}
