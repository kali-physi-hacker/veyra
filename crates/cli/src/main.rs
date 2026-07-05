use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use stratum_domain::*;
use stratum_engine::Engine;

#[derive(Parser)]
#[command(
    name = "stratum",
    version,
    about = "Local system and storage intelligence. Inspection first; actions require immutable plans."
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true, env = "STRATUM_DATA_DIR")]
    data_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[arg(short, long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Status,
    Scan {
        roots: Vec<String>,
        #[arg(long)]
        full: bool,
        #[arg(long)]
        exclude: Vec<String>,
        #[arg(long)]
        ignore: Vec<String>,
        #[arg(long)]
        cross_filesystems: bool,
        #[arg(long)]
        no_hidden: bool,
    },
    Scans,
    Reconcile {
        #[arg(required = true)]
        paths: Vec<String>,
    },
    Files(QueryArgs),
    Find {
        pattern: String,
    },
    Storage {
        #[command(subcommand)]
        command: Option<StorageCommand>,
    },
    Duplicates {
        #[command(subcommand)]
        command: Option<DuplicateCommand>,
    },
    Apps {
        #[command(subcommand)]
        command: Option<AppCommand>,
    },
    Cleanup {
        #[command(subcommand)]
        command: CleanupCommand,
    },
    Insights {
        #[command(subcommand)]
        command: Option<InsightCommand>,
    },
    ExplainStorage,
    System,
    Process {
        #[command(subcommand)]
        command: ProcessCommand,
    },
    Audit {
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u64,
    },
    Api {
        #[command(subcommand)]
        command: ApiCommand,
    },
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
}
#[derive(Subcommand)]
enum StorageCommand {
    Largest(QueryArgs),
    LargestFiles(QueryArgs),
    LargestDirs(QueryArgs),
    Categories,
    History {
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value_t = 0)]
        since: i64,
    },
}
#[derive(Subcommand)]
enum DuplicateCommand {
    Scan,
    Groups {
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u64,
    },
}
#[derive(Subcommand)]
enum AppCommand {
    Inspect { id: String },
    UninstallPlan { id: String },
}
#[derive(Subcommand)]
enum CleanupCommand {
    Analyze(QueryArgs),
    Candidates(QueryArgs),
    Plan {
        #[arg(long, required = true)]
        path: Vec<String>,
    },
    Show {
        id: String,
    },
    Execute {
        id: String,
        #[arg(long)]
        approve: String,
    },
    Undo {
        id: String,
    },
    Operation {
        id: String,
    },
}
#[derive(Subcommand)]
enum InsightCommand {
    Explain { id: String },
}
#[derive(Subcommand)]
enum ProcessCommand {
    Top {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Inspect {
        pid: u32,
    },
}
#[derive(Subcommand)]
enum ApiCommand {
    Serve {
        #[arg(long)]
        bind: Option<String>,
    },
    Token,
    Openapi,
}
#[derive(Subcommand)]
enum DaemonCommand {
    Start {
        #[arg(long)]
        bind: Option<String>,
    },
    Status,
}
#[derive(Args, Default)]
struct QueryArgs {
    #[arg(long)]
    path: Option<String>,
    #[arg(long)]
    parent: Option<String>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    extension: Option<String>,
    #[arg(long)]
    category: Option<String>,
    #[arg(long,value_parser=parse_size)]
    min_size: Option<u64>,
    #[arg(long,value_parser=parse_size)]
    max_size: Option<u64>,
    #[arg(long,value_parser=parse_timestamp)]
    modified_before: Option<i64>,
    #[arg(long,value_parser=parse_timestamp)]
    modified_after: Option<i64>,
    #[arg(long,value_parser=parse_timestamp)]
    created_before: Option<i64>,
    #[arg(long,value_parser=parse_timestamp)]
    created_after: Option<i64>,
    #[arg(long, default_value = "logical_bytes")]
    sort: String,
    #[arg(long, default_value_t = 100)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u64,
}
impl QueryArgs {
    fn query(self, kind: Option<&str>) -> FileQuery {
        FileQuery {
            path: self.path,
            parent: self.parent,
            name: self.name,
            extension: self.extension,
            category: self.category,
            min_size: self.min_size,
            max_size: self.max_size,
            modified_before: self.modified_before,
            modified_after: self.modified_after,
            created_before: self.created_before,
            created_after: self.created_after,
            sort: if self.sort.is_empty() {
                "logical_bytes".into()
            } else {
                self.sort
            },
            limit: if self.limit == 0 { 100 } else { self.limit },
            offset: self.offset,
            kind: kind.map(str::to_owned),
        }
    }
}
fn parse_size(value: &str) -> std::result::Result<u64, String> {
    let upper = value.to_ascii_uppercase();
    let split = upper
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(upper.len());
    let number = upper[..split]
        .parse::<u64>()
        .map_err(|_| "Expected integer size".to_string())?;
    let multiplier = match &upper[split..] {
        "" | "B" => 1,
        "KB" => 1000,
        "MB" => 1000_u64.pow(2),
        "GB" => 1000_u64.pow(3),
        "TB" => 1000_u64.pow(4),
        "KIB" => 1024,
        "MIB" => 1024_u64.pow(2),
        "GIB" => 1024_u64.pow(3),
        "TIB" => 1024_u64.pow(4),
        _ => return Err("Use B, KB, MB, GB, TB or KiB, MiB, GiB, TiB".into()),
    };
    number
        .checked_mul(multiplier)
        .ok_or_else(|| "Size overflow".into())
}
fn parse_timestamp(value: &str) -> std::result::Result<i64, String> {
    if let Ok(epoch) = value.parse::<i64>() {
        return Ok(epoch);
    }
    time::Date::parse(
        value,
        &time::macros::format_description!("[year]-[month]-[day]"),
    )
    .map(|d| d.midnight().assume_utc().unix_timestamp())
    .map_err(|_| "Use YYYY-MM-DD (UTC) or Unix seconds".into())
}
fn value<T: Serialize>(v: T) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(v)?)
}
fn render(v: &serde_json::Value, json: bool) {
    if json {
        println!("{}", v);
        return;
    }
    if let Some(rows) = v
        .get("items")
        .and_then(|v| v.as_array())
        .or_else(|| v.as_array())
        && rows
            .iter()
            .all(|v| v.get("path").is_some() && v.get("logical_bytes").is_some())
    {
        println!("{:>15}  {:>15}  PATH", "LOGICAL BYTES", "ALLOCATED");
        for row in rows {
            println!(
                "{:>15}  {:>15}  {}",
                row["logical_bytes"],
                row["allocated_bytes"],
                row["path"].as_str().unwrap_or("")
            );
        }
        if v["has_more"] == true {
            eprintln!("More results available; increase --offset.");
        }
        return;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
    );
}
#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(if cli.verbose { "stratum=debug" } else { "warn" })
        .init();
    let json = cli.json;
    match run(cli).await {
        Ok(Some(v)) => render(&v, json),
        Ok(None) => {}
        Err(e) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"error":{"code":e.code,"message":e.message}})
                );
            } else {
                eprintln!("{e}");
            }
            std::process::exit(1);
        }
    }
}
async fn run(cli: Cli) -> Result<Option<serde_json::Value>> {
    if matches!(
        cli.command,
        Command::Api {
            command: ApiCommand::Openapi
        }
    ) {
        return Ok(Some(stratum_api::specification()));
    }
    let engine = Engine::open(stratum_engine::load_config(
        cli.config.as_deref(),
        cli.data_dir,
    )?)?;
    if !cli.json && std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        let mut events = engine.subscribe();
        tokio::spawn(async move {
            let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
            loop {
                match events.recv().await {
                    Ok(OperationEvent::ScanProgress { entries, bytes, .. })
                        if last.elapsed().as_secs() >= 1 =>
                    {
                        eprintln!("Indexed {entries} paths · {bytes} file bytes observed");
                        last = std::time::Instant::now();
                    }
                    Ok(OperationEvent::ScanCompleted { scan }) => eprintln!(
                        "Scan {} · {} paths · {} warnings",
                        scan.status, scan.entries, scan.warnings
                    ),
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => {}
                }
            }
        });
    }
    let stop = Arc::new(AtomicBool::new(false));
    let signal_stop = stop.clone();
    let signal_engine = engine.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        signal_stop.store(true, Ordering::Relaxed);
        signal_engine.cancel_all();
    });
    if let Command::Api {
        command: ApiCommand::Serve { bind },
    }
    | Command::Daemon {
        command: DaemonCommand::Start { bind },
    } = &cli.command
    {
        let daemon = matches!(cli.command, Command::Daemon { .. });
        let address = bind
            .as_ref()
            .unwrap_or(&engine.config.api_bind)
            .parse()
            .map_err(|_| Error::invalid("Invalid API bind address"))?;
        let watcher = if daemon {
            let e = engine.clone();
            let s = stop.clone();
            Some(tokio::task::spawn_blocking(move || {
                let result = stratum_engine::daemon::run(e, s.clone());
                if result.is_err() {
                    s.store(true, Ordering::Relaxed);
                }
                result
            }))
        } else {
            None
        };
        let shutdown_stop = stop.clone();
        let service_result = stratum_api::serve(engine.clone(), address, async move {
            while !shutdown_stop.load(Ordering::Relaxed) {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        })
        .await;
        stop.store(true, Ordering::Relaxed);
        engine.cancel_all();
        if let Some(w) = watcher {
            w.await
                .map_err(|e| Error::new("internal_error", e.to_string()))??;
        }
        service_result?;
        return Ok(None);
    }
    let result=tokio::task::spawn_blocking(move||->Result<serde_json::Value>{match cli.command{
        Command::Status=>value(serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"data_dir":engine.config.data_dir,"scans":engine.scans()?,"roots":engine.roots()?})),
        Command::Scan{mut roots,full,exclude,ignore,cross_filesystems,no_hidden}=>{
            if full{if !roots.is_empty(){return Err(Error::invalid("--full cannot be combined with roots"));}roots.push("/".into());}
            if roots.is_empty(){roots=engine.config.scan_roots.clone();if roots.is_empty(){roots.push(std::env::var("HOME").map_err(|_|Error::invalid("Provide a scan root"))?);}}
            value(engine.scan(ScanRequest{roots,exclusions:exclude,ignore_patterns:ignore,include_hidden:!no_hidden,cross_filesystems})?)
        },
        Command::Scans=>value(engine.scans()?),Command::Reconcile{paths}=>value(engine.reconcile_paths(ReconcileRequest{paths})?),Command::Files(q)=>value(engine.files(&q.query(Some("file")))?),Command::Find{pattern}=>value(engine.files(&FileQuery{name:Some(pattern),..Default::default()})?),
        Command::Storage{command:None}|Command::ExplainStorage=>value(engine.explain_storage()?),
        Command::Storage{command:Some(c)}=>match c{StorageCommand::Largest(q)|StorageCommand::LargestFiles(q)=>value(engine.files(&q.query(Some("file")))?),StorageCommand::LargestDirs(q)=>value(engine.files(&q.query(Some("directory")))?),StorageCommand::Categories=>value(engine.categories()?),StorageCommand::History{path,since}=>value(engine.history(path.as_deref(),since)?)},
        Command::Duplicates{command:Some(DuplicateCommand::Scan)}=>value(engine.discover_duplicates(&stop)?),Command::Duplicates{command:None}=>value(engine.duplicates()?),Command::Duplicates{command:Some(DuplicateCommand::Groups{limit,offset})}=>value(engine.duplicate_groups(limit,offset)?),
        Command::Apps{command:None}=>value(engine.applications()?),Command::Apps{command:Some(AppCommand::Inspect{id})}=>value(engine.inspect_application(&id)?),Command::Apps{command:Some(AppCommand::UninstallPlan{id})}=>engine.uninstall_plan(&id),
        Command::Cleanup{command}=>match command{CleanupCommand::Analyze(q)|CleanupCommand::Candidates(q)=>value(engine.cleanup_candidates(&q.query(Some("file")))?),CleanupCommand::Plan{path}=>value(engine.create_cleanup_plan(PlanRequest{paths:path})?),CleanupCommand::Show{id}=>value(engine.cleanup_plan(&id)?),CleanupCommand::Execute{id,approve}=>value(engine.execute_cleanup_plan(&id,&approve)?),CleanupCommand::Undo{id}=>value(engine.undo_cleanup(&id)?),CleanupCommand::Operation{id}=>value(engine.cleanup_operation(&id)?)},
        Command::Insights{command:None}=>value(engine.insights()?),Command::Insights{command:Some(InsightCommand::Explain{id})}=>value(engine.insights()?.into_iter().find(|i|i.id==id).ok_or_else(||Error::new("not_found","Insight not found"))?),
        Command::System=>value(engine.system()),Command::Process{command:ProcessCommand::Top{limit}}=>value(engine.system().processes.into_iter().take(limit).collect::<Vec<_>>()),Command::Process{command:ProcessCommand::Inspect{pid}}=>value(engine.system().processes.into_iter().find(|p|p.pid==pid).ok_or_else(||Error::new("not_found","Process not visible"))?),
        Command::Audit{limit,offset}=>value(engine.audit(limit,offset)?),Command::Api{command:ApiCommand::Token}=>value(serde_json::json!({"token":stratum_api::local_token(&engine)?})),
        Command::Daemon{command:DaemonCommand::Status}=>stratum_api::probe_health(&engine),
        Command::Api{..}|Command::Daemon{..}=>Err(Error::invalid("Unexpected command dispatch")),
    }}).await.map_err(|e|Error::new("internal_error",e.to_string()))??;
    Ok(Some(result))
}
