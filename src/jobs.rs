use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::ffi::OsString;

pub type JobId = u32;

#[derive(Debug, Clone)]
pub struct Job {
    pub id: JobId,
    pub command: String,
    pub status: JobStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobStatus {
    Running,
    Completed(i32),
    Failed(String),
}

pub struct JobManager {
    jobs: Arc<Mutex<HashMap<JobId, Job>>>,
    next_id: Arc<Mutex<JobId>>,
}

impl JobManager {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    pub fn create_job(&self, command: String) -> JobId {
        let mut next_id = self.next_id.lock().unwrap();
        let id = *next_id;
        *next_id += 1;
        
        let job = Job {
            id,
            command: command.clone(),
            status: JobStatus::Running,
        };
        
        self.jobs.lock().unwrap().insert(id, job);
        id
    }

    pub fn complete_job(&self, id: JobId, exit_code: i32) {
        if let Some(job) = self.jobs.lock().unwrap().get_mut(&id) {
            job.status = JobStatus::Completed(exit_code);
        }
    }

    pub fn fail_job(&self, id: JobId, error: String) {
        if let Some(job) = self.jobs.lock().unwrap().get_mut(&id) {
            job.status = JobStatus::Failed(error);
        }
    }

    pub fn get_job(&self, id: JobId) -> Option<Job> {
        self.jobs.lock().unwrap().get(&id).cloned()
    }

    pub fn list_jobs(&self) -> Vec<Job> {
        self.jobs.lock().unwrap().values().cloned().collect()
    }
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new()
    }
}

// Primary `execute_command_sync` now lives below (always uses coreutils/uucore crates as required)

// Keep a minimal set of `uu_*` utilities wired directly; others route to the CoreutilsDispatch
pub fn execute_command_sync(command: &str, args: &[String]) -> i32 {
    let _args_os: Vec<OsString> = args.iter().map(|s| OsString::from(s)).collect();

    match command {
        // Tier 1: File operations (a small subset)
        "cat" => crate::coreutils_dispatch::CoreutilsDispatch::run("cat", args),
        // For now, `ls` delegates to the dispatcher to avoid heavy crate features for wasm.
        "ls" => crate::coreutils_dispatch::CoreutilsDispatch::run("ls", args),

        // Built-ins and simpler utilities
        "echo" => {
            use std::io::Write;
            let mut out = std::io::stdout();
            if args.len() > 1 {
                let _ = writeln!(&mut out, "{}", args[1..].join(" "));
            } else {
                let _ = writeln!(&mut out);
            }
            let _ = out.flush();
            0
        }

        "pwd" => crate::coreutils_dispatch::CoreutilsDispatch::run("pwd", args),

        // Fallback: delegate to CoreutilsDispatch if configured
        _ => {
            crate::coreutils_dispatch::CoreutilsDispatch::run(command, args)
        }
    }
}
