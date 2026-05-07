use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

#[cfg(windows)]
use wait_timeout::ChildExt;

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
#[cfg(windows)]
use windows::core::PCWSTR;

#[derive(Debug, PartialEq, Eq)]
enum HelperMode {
    SelfTest,
    Contain {
        cwd: PathBuf,
        timeout: Duration,
        program: OsString,
        args: Vec<OsString>,
    },
}

fn main() -> ExitCode {
    match real_main() {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("deepseek-windows-sandbox-helper: {err}");
            ExitCode::from(1)
        }
    }
}

fn real_main() -> anyhow::Result<u8> {
    match parse_args(env::args_os().skip(1))? {
        HelperMode::SelfTest => self_test(),
        HelperMode::Contain {
            cwd,
            timeout,
            program,
            args,
        } => run_contained(cwd, timeout, program, args),
    }
}

fn parse_args<I>(args: I) -> anyhow::Result<HelperMode>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        anyhow::bail!("expected --self-test or --contain");
    };

    if first == "--self-test" {
        if args.next().is_some() {
            anyhow::bail!("--self-test does not accept extra arguments");
        }
        return Ok(HelperMode::SelfTest);
    }

    if first != "--contain" {
        anyhow::bail!("unknown mode: {}", first.to_string_lossy());
    }

    let mut cwd = env::current_dir()?;
    let mut timeout = Duration::from_secs(30);

    loop {
        let Some(arg) = args.next() else {
            anyhow::bail!("missing -- before command");
        };

        if arg == "--" {
            break;
        }

        if arg == "--cwd" {
            let Some(value) = args.next() else {
                anyhow::bail!("--cwd requires a value");
            };
            cwd = PathBuf::from(value);
            continue;
        }

        if arg == "--timeout-ms" {
            let Some(value) = args.next() else {
                anyhow::bail!("--timeout-ms requires a value");
            };
            let timeout_ms = value
                .to_string_lossy()
                .parse::<u64>()
                .map_err(|_| anyhow::anyhow!("invalid --timeout-ms value"))?;
            timeout = Duration::from_millis(timeout_ms);
            continue;
        }

        anyhow::bail!("unknown option before command: {}", arg.to_string_lossy());
    }

    let Some(program) = args.next() else {
        anyhow::bail!("missing command after --");
    };

    Ok(HelperMode::Contain {
        cwd,
        timeout,
        program,
        args: args.collect(),
    })
}

#[cfg(windows)]
fn self_test() -> anyhow::Result<u8> {
    let _job = JobObject::create_kill_on_close()?;
    Ok(0)
}

#[cfg(not(windows))]
fn self_test() -> anyhow::Result<u8> {
    anyhow::bail!("Windows sandbox helper is only supported on Windows")
}

#[cfg(windows)]
fn run_contained(
    cwd: PathBuf,
    timeout: Duration,
    program: OsString,
    args: Vec<OsString>,
) -> anyhow::Result<u8> {
    let job = JobObject::create_kill_on_close()?;
    let mut child = Command::new(&program)
        .args(&args)
        .current_dir(cwd)
        .spawn()
        .map_err(|err| {
            anyhow::anyhow!(
                "failed to spawn {}: {err}",
                PathBuf::from(&program).display()
            )
        })?;

    if let Err(err) = job.assign_process(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(err);
    }

    let status = match child.wait_timeout(timeout)? {
        Some(status) => status,
        None => {
            let _ = job.terminate(124);
            let _ = child.wait();
            return Ok(124);
        }
    };

    Ok(status.code().unwrap_or(1).try_into().unwrap_or(1))
}

#[cfg(not(windows))]
fn run_contained(
    _cwd: PathBuf,
    _timeout: Duration,
    _program: OsString,
    _args: Vec<OsString>,
) -> anyhow::Result<u8> {
    anyhow::bail!("Windows sandbox helper is only supported on Windows")
}

#[cfg(windows)]
struct JobObject(HANDLE);

#[cfg(windows)]
impl JobObject {
    fn create_kill_on_close() -> anyhow::Result<Self> {
        let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }?;

        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )?;
        }

        Ok(Self(job))
    }

    fn assign_process(&self, child: &std::process::Child) -> anyhow::Result<()> {
        let process = HANDLE(child.as_raw_handle());
        unsafe { AssignProcessToJobObject(self.0, process) }?;
        Ok(())
    }

    fn terminate(&self, exit_code: u32) -> anyhow::Result<()> {
        unsafe { TerminateJobObject(self.0, exit_code) }?;
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for JobObject {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_self_test() {
        assert_eq!(
            parse_args([OsString::from("--self-test")]).unwrap(),
            HelperMode::SelfTest
        );
    }

    #[test]
    fn parse_contain_command() {
        let parsed = parse_args([
            OsString::from("--contain"),
            OsString::from("--cwd"),
            OsString::from("C:\\repo"),
            OsString::from("--timeout-ms"),
            OsString::from("1234"),
            OsString::from("--"),
            OsString::from("cmd.exe"),
            OsString::from("/C"),
            OsString::from("echo hello"),
        ])
        .unwrap();

        assert_eq!(
            parsed,
            HelperMode::Contain {
                cwd: PathBuf::from("C:\\repo"),
                timeout: Duration::from_millis(1234),
                program: OsString::from("cmd.exe"),
                args: vec![OsString::from("/C"), OsString::from("echo hello")],
            }
        );
    }
}
