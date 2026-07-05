use crate::{
    Engine,
    intelligence::{CleanupRule, RegeneratableRule},
};
use std::{
    fs,
    path::{Component, Path},
};
use stratum_domain::*;
use stratum_platform::{
    scanner::identity,
    secure_fs::{hash_file, open_regular, rename_no_replace},
};

impl Engine {
    pub(crate) fn cleanup_path_allowed(&self, path: &Path) -> Result<()> {
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(Error::new(
                "protected_path",
                "Cleanup paths must be absolute without traversal components",
            ));
        }
        let builtin = [
            "/System",
            "/Library",
            "/usr",
            "/bin",
            "/sbin",
            "/etc",
            "/private/etc",
            "/private/var/db",
            "/private/var/root",
            "/dev",
            "/proc",
            "/sys",
            "/run",
            "/boot",
            "/root",
        ];
        if path.parent().is_none()
            || builtin.iter().any(|p| path.starts_with(p))
            || path.starts_with(&self.config.data_dir)
            || self
                .config
                .protected_paths
                .iter()
                .any(|p| path.starts_with(p))
            || path.components().any(|c| {
                [".ssh", ".gnupg", ".git", "Keychains", "keychains"]
                    .iter()
                    .any(|n| c.as_os_str() == *n)
            })
        {
            return Err(Error::new(
                "protected_path",
                "Path is protected by cleanup policy",
            ));
        }
        let allowed = if self.config.allowed_cleanup_roots.is_empty() {
            self.store
                .roots()?
                .into_iter()
                .map(std::path::PathBuf::from)
                .collect()
        } else {
            self.config.allowed_cleanup_roots.clone()
        };
        if !allowed.iter().any(|p| path.starts_with(p) && path != p) {
            return Err(Error::new(
                "protected_path",
                "Path is outside allowed indexed cleanup roots",
            ));
        }
        Ok(())
    }
    pub fn create_cleanup_plan(&self, request: PlanRequest) -> Result<CleanupPlan> {
        let _guard = self.writer_lock()?;
        if request.paths.is_empty() || request.paths.len() > 1000 {
            return Err(Error::invalid(
                "Select 1..1000 explicit candidate file paths",
            ));
        }
        let mut paths = request.paths;
        paths.sort();
        paths.dedup();
        let mut items = vec![];
        let quarantine = self.quarantine_dir()?;
        let quarantine_device = identity(&fs::metadata(&quarantine)?).device;
        for path in paths {
            self.cleanup_path_allowed(Path::new(&path))?;
            let entry = self.store.entry(&path)?;
            let candidate=RegeneratableRule.evaluate(&entry).ok_or_else(||Error::new("invalid_cleanup_plan","Only recognized Cargo build artifacts and package-cache regular files can be quarantined in v0.1"))?;
            let (hash, current) = hash_file(Path::new(&path), false, || false)?;
            if current != entry.identity {
                return Err(Error::new(
                    "filesystem_changed",
                    format!("Rescan changed path: {path}"),
                ));
            }
            if current.links != 1 {
                return Err(Error::new(
                    "invalid_cleanup_plan",
                    "Hard-linked files are not cleanup candidates",
                ));
            }
            if current.device != quarantine_device {
                return Err(Error::new(
                    "unsupported_platform_feature",
                    "Cross-device quarantine is unsupported; configure data_dir on the same filesystem",
                ));
            }
            items.push(PlanItem {
                path,
                bytes: current.size,
                identity: current,
                content_hash: hash,
                reason: candidate.reason,
                risk: candidate.risk,
            });
        }
        let plan_id = id();
        let plan = CleanupPlan {
            id: plan_id.clone(),
            created_at: now(),
            expires_at: now() + 86400,
            total_bytes: items.iter().map(|i| i.bytes).sum(),
            items,
            action: "quarantine_regular_files".into(),
            risk: "moderate".into(),
            approval_phrase: format!("QUARANTINE {plan_id}"),
        };
        self.store.put("plan", &plan.id, &plan)?;
        self.store.audit(
            "cleanup_plan_created",
            &plan.id,
            &format!("{} files, {} bytes", plan.items.len(), plan.total_bytes),
        )?;
        Ok(plan)
    }
    fn quarantine_dir(&self) -> Result<std::path::PathBuf> {
        let path = self.config.data_dir.join("quarantine");
        if !path.exists() {
            fs::create_dir(&path)?;
        }
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(Error::new(
                "protected_path",
                "Quarantine cannot be a symlink",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        }
        Ok(path)
    }
    pub fn cleanup_plan(&self, id: &str) -> Result<CleanupPlan> {
        self.store.get("plan", id)
    }
    pub fn cleanup_operation(&self, id: &str) -> Result<CleanupOperation> {
        self.store.get("operation", id)
    }
    pub fn execute_cleanup_plan(&self, plan_id: &str, approval: &str) -> Result<CleanupOperation> {
        let _guard = self.writer_lock()?;
        let plan = self.cleanup_plan(plan_id)?;
        if approval != plan.approval_phrase {
            return Err(Error::new(
                "approval_required",
                format!("Explicit approval must equal {}", plan.approval_phrase),
            ));
        }
        if now() > plan.expires_at {
            return Err(Error::new(
                "invalid_cleanup_plan",
                "Plan expired; create and review a new plan",
            ));
        }
        match self.store.get::<String>("executed_plan", plan_id) {
            Ok(_) => {
                return Err(Error::new(
                    "conflict",
                    "Plan has already been claimed; inspect its operation instead",
                ));
            }
            Err(error) if error.code == "not_found" => {}
            Err(error) => return Err(error),
        }
        // Validate the entire immutable selection before touching the first source.
        for item in &plan.items {
            self.cleanup_path_allowed(Path::new(&item.path))?;
            let (hash, current) = hash_file(Path::new(&item.path), false, || false)?;
            if current != item.identity || hash != item.content_hash {
                return Err(Error::new(
                    "filesystem_changed",
                    format!("Plan invalidated by change to {}", item.path),
                ));
            }
        }
        let operation_id = id();
        let dir = self.quarantine_dir()?.join(&operation_id);
        fs::create_dir(&dir)?;
        let mut operation = CleanupOperation {
            id: operation_id.clone(),
            plan_id: plan_id.into(),
            created_at: now(),
            status: "running".into(),
            items: plan
                .items
                .iter()
                .enumerate()
                .map(|(i, p)| QuarantineItem {
                    source: p.path.clone(),
                    destination: dir.join(format!("{i:06}")).to_string_lossy().into(),
                    identity: p.identity.clone(),
                    hash: p.content_hash.clone(),
                    status: "pending".into(),
                    error: None,
                })
                .collect(),
        };
        self.store.put("operation", &operation.id, &operation)?;
        self.store.put("executed_plan", plan_id, &operation.id)?;
        self.store
            .audit("cleanup_started", &operation.id, plan_id)?;
        self.emit(OperationEvent::CleanupStarted {
            operation_id: operation.id.clone(),
        });
        for i in 0..operation.items.len() {
            operation.items[i].status = "moving".into();
            self.store.put("operation", &operation.id, &operation)?;
            let item = &mut operation.items[i];
            let action = (|| -> Result<()> {
                let current = identity(&open_regular(Path::new(&item.source))?.metadata()?);
                if current != item.identity {
                    return Err(Error::new(
                        "filesystem_changed",
                        "File identity changed after preflight",
                    ));
                }
                rename_no_replace(Path::new(&item.source), Path::new(&item.destination))?;
                let (hash, moved) = match hash_file(Path::new(&item.destination), false, || false) {
                    Ok(value) => value,
                    Err(error) => {
                        let restored = rename_no_replace(
                            Path::new(&item.destination),
                            Path::new(&item.source),
                        );
                        return Err(Error::new(
                            "filesystem_changed",
                            format!(
                                "Moved object could not be verified: {error}; immediate rollback: {restored:?}"
                            ),
                        ));
                    }
                };
                // Rename may update ctime. Device/inode, mtime, size, link count and bytes must match.
                if hash != item.hash
                    || moved.device != item.identity.device
                    || moved.inode != item.identity.inode
                    || moved.size != item.identity.size
                    || moved.modified_ns != item.identity.modified_ns
                    || moved.links != 1
                {
                    let restored =
                        rename_no_replace(Path::new(&item.destination), Path::new(&item.source));
                    return Err(Error::new(
                        "filesystem_changed",
                        format!("Moved file failed verification; immediate rollback: {restored:?}"),
                    ));
                }
                item.identity = moved;
                Ok(())
            })();
            match action {
                Ok(()) => item.status = "quarantined".into(),
                Err(e) => {
                    item.status = if fs::symlink_metadata(&item.destination).is_ok() {
                        "needs_review"
                    } else {
                        "failed"
                    }
                    .into();
                    item.error = Some(e.to_string());
                }
            }
            self.store.audit(
                "cleanup_item",
                &operation.id,
                &format!("{}: {}", item.source, item.status),
            )?;
            self.emit(OperationEvent::CleanupProgress {
                operation_id: operation.id.clone(),
                path: item.source.clone(),
                status: item.status.clone(),
            });
            let failed = item.status != "quarantined";
            self.store.put("operation", &operation.id, &operation)?;
            if failed {
                break;
            }
        }
        operation.status = if operation.items.iter().all(|i| i.status == "quarantined") {
            "completed"
        } else {
            "partial"
        }
        .into();
        self.store.put("operation", &operation.id, &operation)?;
        self.store
            .audit("cleanup_completed", &operation.id, &operation.status)?;
        for root in self.store.roots()? {
            self.store.mark_stale(&root)?;
        }
        self.emit(OperationEvent::CleanupCompleted {
            operation_id: operation.id.clone(),
            status: operation.status.clone(),
        });
        Ok(operation)
    }
    pub fn undo_cleanup(&self, operation_id: &str) -> Result<CleanupOperation> {
        let _guard = self.writer_lock()?;
        let mut operation = self.cleanup_operation(operation_id)?;
        self.store.audit(
            "restore_started",
            operation_id,
            "No-clobber restore requested",
        )?;
        for i in 0..operation.items.len() {
            let item = &mut operation.items[i];
            if item.status == "restored" || item.status == "pending" || item.status == "failed" {
                continue;
            }
            let result = (|| -> Result<()> {
                self.cleanup_path_allowed(Path::new(&item.source))?;
                if fs::symlink_metadata(&item.destination)
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                {
                    let (hash, current) = hash_file(Path::new(&item.source), false, || false)?;
                    if hash == item.hash
                        && current.device == item.identity.device
                        && current.inode == item.identity.inode
                        && current.size == item.identity.size
                    {
                        return Ok(());
                    }
                    return Err(Error::new(
                        "filesystem_changed",
                        "Quarantine is absent and the source does not match the recorded object",
                    ));
                }
                // Never follow replaced destinations or overwrite an existing original.
                let (hash, current) = hash_file(Path::new(&item.destination), false, || false)?;
                if hash != item.hash
                    || current.device != item.identity.device
                    || current.inode != item.identity.inode
                    || current.size != item.identity.size
                {
                    return Err(Error::new(
                        "filesystem_changed",
                        "Quarantine object changed; manual review required",
                    ));
                }
                rename_no_replace(Path::new(&item.destination), Path::new(&item.source))?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    item.status = "restored".into();
                    item.error = None;
                }
                Err(e) => {
                    item.status = "restore_failed".into();
                    item.error = Some(e.to_string());
                }
            }
            self.store.audit(
                "restore_item",
                operation_id,
                &format!("{}: {}", item.source, item.status),
            )?;
            self.store.put("operation", operation_id, &operation)?;
        }
        operation.status = if operation
            .items
            .iter()
            .all(|i| ["restored", "failed", "pending"].contains(&i.status.as_str()))
        {
            "restored"
        } else {
            "restore_partial"
        }
        .into();
        self.store.put("operation", operation_id, &operation)?;
        self.store
            .audit("restore_completed", operation_id, &operation.status)?;
        for root in self.store.roots()? {
            self.store.mark_stale(&root)?;
        }
        Ok(operation)
    }
}
