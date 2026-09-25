//! Linux PTY ownership: one nonblocking worker per session; no raw descriptors
//! or process IDs are accepted from the frontend.
use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{process::CommandExt, process::ExitStatusExt},
    },
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
};

use super::{TerminalCommand, TerminalEvent};

const CHUNK: usize = 8192;
const OUTPUT_WINDOW: usize = 8;

pub(super) struct PtyProcess {
    master: File,
    child: Child,
    reaped: bool,
}

impl PtyProcess {
    pub(super) fn spawn(root: &Path, cols: u16, rows: u16) -> io::Result<Self> {
        // Require identity-safe process signaling (Linux >= 5.3) before spawn.
        drop(pidfd(unsafe { libc::getpid() })?);
        // All parent descriptors are CLOEXEC at creation, including under
        // concurrent process launches elsewhere in the application.
        let fd = unsafe {
            libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC | libc::O_NONBLOCK)
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let master = unsafe { File::from_raw_fd(fd) };
        if unsafe { libc::grantpt(fd) } != 0 || unsafe { libc::unlockpt(fd) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut name = [0 as libc::c_char; 256];
        let error = unsafe { libc::ptsname_r(fd, name.as_mut_ptr(), name.len()) };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error));
        }
        let slave_fd = unsafe {
            libc::open(
                name.as_ptr(),
                libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
            )
        };
        if slave_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let slave = unsafe { File::from_raw_fd(slave_fd) };
        resize(&master, cols, rows)?;
        let mut command = Command::new("/bin/bash");
        command
            .args(["--noprofile", "--norc", "-i"])
            .current_dir(root)
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        // Only async-signal-safe syscalls between fork and exec. Stdio has
        // already been duped by Command when this hook runs.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        Ok(Self {
            master,
            child,
            reaped: false,
        })
    }

    pub(super) fn run(
        mut self,
        id: String,
        receiver: mpsc::Receiver<TerminalCommand>,
        stop: Arc<AtomicBool>,
        pending_input: Arc<AtomicUsize>,
        sink: impl Fn(TerminalEvent) -> bool,
    ) {
        let mut ready = false;
        let mut sequence = 0_u64;
        let mut unacked = VecDeque::new();
        let mut input = VecDeque::new();
        let mut buffer = [0_u8; CHUNK];
        let mut shell_done = false;
        let mut offset = 0;
        'session: while !stop.load(Ordering::Acquire) {
            // Limit work per iteration so floods cannot starve stop/PTY I/O.
            for _ in 0..64 {
                match receiver.try_recv() {
                    Ok(TerminalCommand::Input(data)) => input.push_back(data),
                    Ok(TerminalCommand::Resize(cols, rows)) => {
                        if resize(&self.master, cols, rows).is_err() {
                            break 'session;
                        }
                    }
                    Ok(TerminalCommand::Ack(ack)) => {
                        if ack == 0 {
                            ready = true;
                        }
                        if ack <= sequence {
                            while unacked.front().is_some_and(|value| *value <= ack) {
                                unacked.pop_front();
                            }
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => break 'session,
                }
            }
            if !shell_done && self.exited() {
                shell_done = true;
                self.kill_session();
            }
            if ready && let Some(data) = input.front() {
                match self.master.write(&data[offset..]) {
                    Ok(0) => break,
                    Ok(count) => {
                        pending_input.fetch_sub(count, Ordering::AcqRel);
                        offset += count;
                        if offset == data.len() {
                            input.pop_front();
                            offset = 0;
                        }
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => break,
                }
            }
            let can_read = ready && unacked.len() < OUTPUT_WINDOW;
            if can_read {
                match self.master.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        sequence += 1;
                        unacked.push_back(sequence);
                        if !sink(TerminalEvent::Output {
                            session_id: id.clone(),
                            sequence,
                            data: buffer[..count].to_vec(),
                        }) {
                            break;
                        }
                        continue;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if shell_done {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    // Linux reports EIO when the last slave descriptor closes.
                    Err(_) => break,
                }
            }
            let events = (if can_read { libc::POLLIN } else { 0 })
                | (if !input.is_empty() && ready {
                    libc::POLLOUT
                } else {
                    0
                });
            let mut poll = libc::pollfd {
                fd: self.master.as_raw_fd(),
                events,
                revents: 0,
            };
            // With no requested events use a timer; HUP would otherwise spin
            // while the frontend is still acknowledging the output window.
            unsafe {
                libc::poll(
                    if events == 0 {
                        std::ptr::null_mut()
                    } else {
                        &mut poll
                    },
                    if events == 0 { 0 } else { 1 },
                    20,
                );
            }
        }
        self.kill_session();
        let status = self.child.wait().ok();
        self.reaped = true;
        let _ = sink(TerminalEvent::Exit {
            session_id: id,
            exit_code: status.and_then(|value| value.code()),
            signal: status.and_then(|value| value.signal()),
        });
    }

    fn exited(&self) -> bool {
        // WNOWAIT keeps the leader PID reserved until all job groups have been
        // cleaned up, even if the interactive shell exited first.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            ) == 0
                && info.si_pid() != 0
        }
    }

    fn kill_session(&self) {
        let sid = self.child.id() as i32;
        unsafe {
            libc::kill(sid, libc::SIGSTOP);
        }
        // Interactive job control creates separate groups for background and
        // foreground jobs. Freeze them and rescan for in-flight child forks.
        let mut members = BTreeMap::new();
        for _ in 0..3 {
            if let Ok(entries) = std::fs::read_dir("/proc") {
                for entry in entries.flatten() {
                    let Some(pid) = entry
                        .file_name()
                        .to_str()
                        .and_then(|name| name.parse::<i32>().ok())
                    else {
                        continue;
                    };
                    let Ok(handle) = pidfd(pid) else {
                        continue;
                    };
                    if unsafe { libc::getsid(pid) } != sid {
                        continue;
                    }
                    signal_pidfd(&handle, libc::SIGSTOP);
                    members.insert(pid, handle);
                }
            }
        }
        for handle in members.values() {
            signal_pidfd(handle, libc::SIGKILL);
        }
        unsafe {
            libc::kill(sid, libc::SIGKILL);
        }
    }
}

fn pidfd(pid: i32) -> io::Result<File> {
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd as i32) })
    }
}

fn signal_pidfd(handle: &File, signal: i32) {
    // A pidfd refers to the original process even if its numeric PID was
    // reaped/reused between discovery, authorization and this syscall.
    unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            handle.as_raw_fd(),
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        );
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        if !self.reaped {
            self.kill_session();
            let _ = self.child.wait();
        }
    }
}

fn resize(master: &File, cols: u16, rows: u16) -> io::Result<()> {
    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &size) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
