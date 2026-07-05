use crate::Engine;
use std::sync::Arc;
use stratum_domain::*;
impl Engine {
    pub(crate) fn recover_jobs(&self) -> Result<()> {
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.config.data_dir.join("job.lock"))?;
        if lock.try_lock().is_ok() {
            for mut job in self.store.documents::<Job>("job")? {
                if job.status == "running" {
                    job.status = "interrupted".into();
                    job.error = Some(
                        serde_json::json!({"code":"interrupted","message":"Process stopped before job completion; inspect scan and operation records"}),
                    );
                    self.store.put("job", &job.id, &job)?;
                }
            }
        }
        Ok(())
    }
    pub fn start_scan_job(self: &Arc<Self>, request: ScanRequest) -> Result<Job> {
        if request.roots.len() > 16 {
            return Err(Error::invalid("At most 16 roots per scan job"));
        }
        self.spawn_job(move |e| Ok(serde_json::to_value(e.scan(request)?)?))
    }
    pub fn start_duplicate_job(self: &Arc<Self>) -> Result<Job> {
        self.spawn_job(|e| {
            Ok(serde_json::to_value(e.discover_duplicates(
                &std::sync::atomic::AtomicBool::new(false),
            )?)?)
        })
    }
    pub fn job(&self, id: &str) -> Result<Job> {
        self.store.get("job", id)
    }
    fn spawn_job(
        self: &Arc<Self>,
        work: impl FnOnce(Arc<Engine>) -> Result<serde_json::Value> + Send + 'static,
    ) -> Result<Job> {
        // A separate cross-process job lock bounds asynchronous submissions to one active job.
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.config.data_dir.join("job.lock"))?;
        lock.try_lock()
            .map_err(|_| Error::new("busy", "A background analysis job is already running"))?;
        let job = Job {
            id: id(),
            status: "running".into(),
            result: None,
            error: None,
        };
        self.store.put("job", &job.id, &job)?;
        let mut background = job.clone();
        let engine = self.clone();
        std::thread::spawn(move || {
            let _lock = lock;
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(engine.clone())))
                    .unwrap_or_else(|_| {
                        Err(Error::new("internal_error", "Background worker panicked"))
                    });
            match result {
                Ok(v) => {
                    background.status = "completed".into();
                    background.result = Some(v);
                }
                Err(e) => {
                    background.status = "failed".into();
                    background.error = Some(serde_json::json!({"code":e.code,"message":e.message}));
                }
            }
            if let Err(e) = engine.store.put("job", &background.id, &background) {
                tracing::error!(error=%e,"Failed to persist job completion");
            }
        });
        Ok(job)
    }
}
