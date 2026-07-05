use stratum_domain::*;
use sysinfo::{Disks, System};
pub fn snapshot() -> SystemSnapshot {
    let mut system = System::new_all();
    let started = std::time::Instant::now();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    system.refresh_all();
    let load = System::load_average();
    let disks = Disks::new_with_refreshed_list();
    let volumes = disks
        .iter()
        .map(|d| Volume {
            name: d.name().to_string_lossy().into(),
            mount: d.mount_point().to_string_lossy().into(),
            filesystem: d.file_system().to_string_lossy().into(),
            total_bytes: d.total_space(),
            available_bytes: d.available_space(),
            removable: d.is_removable(),
        })
        .collect();
    let mut processes: Vec<_> = system
        .processes()
        .iter()
        .map(|(pid, p)| {
            let io = p.disk_usage();
            ProcessSnapshot {
                pid: pid.as_u32(),
                parent_pid: p.parent().map(|p| p.as_u32()),
                name: p.name().to_string_lossy().into(),
                cpu_percent: p.cpu_usage(),
                memory_bytes: p.memory(),
                disk_read_bytes: io.total_read_bytes,
                disk_written_bytes: io.total_written_bytes,
                started_at: p.start_time(),
                runtime_seconds: p.run_time(),
            }
        })
        .collect();
    processes.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));
    SystemSnapshot {
        timestamp: now(),
        cpu_percent: system.global_cpu_usage(),
        load_average: vec![load.one, load.five, load.fifteen],
        total_memory: system.total_memory(),
        used_memory: system.used_memory(),
        total_swap: system.total_swap(),
        used_swap: system.used_swap(),
        memory_pressure: None,
        volumes,
        processes,
        sample_millis: started.elapsed().as_millis() as u64,
        limitations: vec![
            "Memory pressure unavailable; process visibility depends on OS permissions".into(),
            "Process I/O counters are OS-reported cumulative bytes, not physical volume traffic"
                .into(),
        ],
    }
}
