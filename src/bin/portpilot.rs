use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use comfy_table::{Cell, ContentArrangement, Table, presets::UTF8_FULL};
use crossterm::event::KeyModifiers;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use portpilot::{
    PortPilot,
    models::{PortBinding, TerminationTarget},
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{
    Block, Borders, Cell as TuiCell, Paragraph, Row, Table as TuiTable, TableState,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(name = "portpilot", version, about = "Find it. Inspect it. Kill it.")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    List {
        #[arg(long)]
        search: Option<String>,
    },
    Port {
        port: u16,
    },
    Process {
        query: String,
    },
    /// Kill a process by port or PID (asks for confirmation first)
    Kill {
        target: String,
        /// Treat the target as a port
        #[arg(long, conflicts_with = "pid")]
        port: bool,
        /// Treat the target as a PID
        #[arg(long)]
        pid: bool,
    },
    Inspect {
        pid: u32,
    },
    Watch {
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
    },
    Dev {
        #[command(subcommand)]
        command: Option<DevCommand>,
    },
}

#[derive(Debug, Subcommand)]
enum DevCommand {
    Add { service: String },
    Remove { service: String },
    Up,
    Down,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let pilot = PortPilot::default();

    match cli.command.unwrap_or(Command::List { search: None }) {
        Command::List { search } => {
            let mut bindings = pilot.list_ports()?;
            if let Some(search) = search {
                let needle = search.to_ascii_lowercase();
                bindings.retain(|binding| {
                    binding.process.to_ascii_lowercase().contains(&needle)
                        || binding.pid.to_string().contains(&needle)
                        || binding.port.to_string().contains(&needle)
                });
            }

            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header([
                    "PORT", "PROTOCOL", "PID", "PROCESS", "CPU", "MEMORY", "STATUS",
                ]);
            for binding in bindings {
                table.add_row([
                    Cell::new(binding.port),
                    Cell::new(format!("{:?}", binding.protocol).to_uppercase()),
                    Cell::new(binding.pid),
                    Cell::new(binding.process),
                    Cell::new(format!("{:.1}%", binding.cpu_percent)),
                    Cell::new(format_bytes(binding.memory_bytes)),
                    Cell::new(binding.status.unwrap_or_else(|| "-".to_string())),
                ]);
            }
            println!("{table}");
        }
        Command::Port { port } => {
            let binding = pilot.find_by_port(port)?;
            println!("Port:       {}", binding.port);
            println!(
                "Protocol:   {}",
                format!("{:?}", binding.protocol).to_uppercase()
            );
            println!("PID:        {}", binding.pid);
            println!("Process:    {}", binding.process);
            println!("CPU:        {:.1}%", binding.cpu_percent);
            println!("Memory:     {}", format_bytes(binding.memory_bytes));
            if let Some(status) = binding.status {
                println!("Status:     {status}");
            }
        }
        Command::Process { query } => {
            let bindings = pilot.find_by_process(&query)?;
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(["PID", "PROCESS", "PORT", "PROTOCOL", "CPU", "MEMORY"]);
            for binding in bindings {
                table.add_row([
                    Cell::new(binding.pid),
                    Cell::new(binding.process),
                    Cell::new(binding.port),
                    Cell::new(format!("{:?}", binding.protocol).to_uppercase()),
                    Cell::new(format!("{:.1}%", binding.cpu_percent)),
                    Cell::new(format_bytes(binding.memory_bytes)),
                ]);
            }
            println!("{table}");
        }
        Command::Inspect { pid } => {
            let details = pilot.inspect_pid(pid)?;
            println!("PID:       {}", details.pid);
            println!("Process:   {}", details.process);
            println!("Command:   {}", details.command);
            println!("CPU:       {:.1}%", details.cpu_percent);
            println!("Memory:    {}", format_bytes(details.memory_bytes));
            println!(
                "User:      {}",
                details.user.unwrap_or_else(|| "<unknown>".to_string())
            );
            if details.ports.is_empty() {
                println!("Ports:     -");
            } else {
                println!(
                    "Ports:     {}",
                    details
                        .ports
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        Command::Kill { target, port, pid } => {
            let target_num = target.parse::<u32>()?;
            let termination_target = resolve_kill_target(&pilot, target_num, port, pid)?;

            let pid = match termination_target {
                TerminationTarget::Port(port) => pilot.find_by_port(port)?.pid,
                TerminationTarget::Pid(pid) => pid,
            };
            let process = pilot.inspect_pid(pid)?;

            println!("Target PID: {}", pid);
            println!("Process:    {}", process.process);
            println!("Command:    {}", process.command);
            print!("Kill this process? [y/N] ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if input.trim().eq_ignore_ascii_case("y") {
                let result = pilot.terminate(termination_target)?;
                println!("✓ Process {} terminated", result.pid);
                if !result.freed_ports.is_empty() {
                    let mut freed_ports = result.freed_ports;
                    freed_ports.sort_unstable();
                    println!(
                        "✓ Freed ports: {}",
                        freed_ports
                            .iter()
                            .map(u16::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            } else {
                println!("Aborted");
            }
        }
        Command::Watch { interval_ms } => run_watch_tui(&pilot, interval_ms)?,
        Command::Dev { command } => run_dev_command(&pilot, command)?,
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PortPilotConfig {
    dev: DevConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DevConfig {
    services: Vec<DevService>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DevService {
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Catalog {
    services: Vec<CatalogService>,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogService {
    name: String,
    image: String,
    default_port: u16,
    #[serde(default)]
    env: Vec<String>,
}

const CATALOG_FILE_NAME: &str = "catalog.toml";
const DEFAULT_CATALOG: &str = include_str!("../../catalog.toml");

const CONFIG_FILE_NAME: &str = "portpilot.toml";

fn resolve_kill_target(
    pilot: &PortPilot,
    target: u32,
    force_port: bool,
    force_pid: bool,
) -> Result<TerminationTarget> {
    let as_port = u16::try_from(target)
        .ok()
        .filter(|port| pilot.find_by_port(*port).is_ok());
    if force_pid {
        return Ok(TerminationTarget::Pid(target));
    }
    if force_port {
        let port = u16::try_from(target).context("port must be between 0 and 65535")?;
        return Ok(TerminationTarget::Port(port));
    }
    match as_port {
        Some(_) if pilot.inspect_pid(target).is_ok() => Err(anyhow!(
            "{target} is both a listening port and a PID; re-run with --port or --pid"
        )),
        Some(port) => Ok(TerminationTarget::Port(port)),
        None => Ok(TerminationTarget::Pid(target)),
    }
}

fn run_dev_command(pilot: &PortPilot, command: Option<DevCommand>) -> Result<()> {
    match command {
        None => run_dev_status(pilot),
        Some(DevCommand::Add { service }) => run_dev_add(service),
        Some(DevCommand::Remove { service }) => run_dev_remove(service),
        Some(DevCommand::Up) => run_dev_up(),
        Some(DevCommand::Down) => run_dev_down(),
    }
}

fn run_dev_status(pilot: &PortPilot) -> Result<()> {
    let config = load_or_create_config()?;
    let catalog = load_catalog()?;
    print_dev_services(pilot, &config.dev.services, &catalog);
    print_dev_commands();
    Ok(())
}

fn run_dev_add(service: String) -> Result<()> {
    let mut config = load_or_create_config()?;
    let catalog = load_catalog()?;
    let service = normalize_service_name(&service)?;
    let entry = catalog_entry(&catalog, &service)?;
    if config
        .dev
        .services
        .iter()
        .any(|item| item.name.eq_ignore_ascii_case(&service))
    {
        return Err(anyhow!(
            "service '{service}' already exists in {CONFIG_FILE_NAME}"
        ));
    }

    config.dev.services.push(DevService {
        name: service.clone(),
    });
    write_config(&config)?;
    println!(
        "Added service '{service}' ({}) to {CONFIG_FILE_NAME}",
        entry.image
    );
    Ok(())
}

fn run_dev_remove(service: String) -> Result<()> {
    let mut config = load_or_create_config()?;
    let service = normalize_service_name(&service)?;
    let before = config.dev.services.len();
    config
        .dev
        .services
        .retain(|item| !item.name.eq_ignore_ascii_case(&service));

    if config.dev.services.len() == before {
        return Err(anyhow!(
            "service '{service}' was not found in {CONFIG_FILE_NAME}"
        ));
    }

    write_config(&config)?;
    println!("Removed service '{service}' from {CONFIG_FILE_NAME}");
    Ok(())
}

fn run_dev_up() -> Result<()> {
    let config = load_or_create_config()?;
    let catalog = load_catalog()?;
    let entries = resolve_entries(&config, &catalog)?;

    let (tx, rx) = std::sync::mpsc::channel();
    let total = entries.len();
    for entry in entries {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let name = entry.name.clone();
            let _ = tx.send((name, start_service(&entry)));
        });
    }
    drop(tx);

    let mut failures = Vec::new();
    for (name, result) in rx.iter().take(total) {
        match result {
            Ok(message) => println!("● {message}"),
            Err(err) => {
                println!("✗ {name} failed");
                failures.push(format!("{name}: {err:#}"));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "some services failed:\n  {}",
            failures.join("\n  ")
        ))
    }
}

fn start_service(entry: &CatalogService) -> Result<String> {
    let container = container_name(&entry.name);
    match container_state(&container)?.as_deref() {
        Some("running") => Ok(format!("{} already running", entry.name)),
        Some(_) => {
            docker(&["start", &container])?;
            verify_running(&entry.name, &container)?;
            Ok(format!("{} started ({})", entry.name, entry.image))
        }
        None => {
            if !image_exists(&entry.image)? {
                println!("↓ {} pulling {}…", entry.name, entry.image);
                docker(&["pull", "-q", &entry.image])?;
            }
            let mapping = format!("{0}:{0}", entry.default_port);
            let mut args = vec!["run", "-d", "--name", &container, "-p", &mapping];
            for var in &entry.env {
                args.extend(["-e", var]);
            }
            args.push(&entry.image);
            docker(&args)?;
            verify_running(&entry.name, &container)?;
            Ok(format!(
                "{} started ({}) on :{}",
                entry.name, entry.image, entry.default_port
            ))
        }
    }
}

fn image_exists(image: &str) -> Result<bool> {
    let status = std::process::Command::new("docker")
        .args(["image", "inspect", image])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("failed to run docker; is Docker installed and running?")?;
    Ok(status.success())
}

fn verify_running(name: &str, container: &str) -> Result<()> {
    std::thread::sleep(Duration::from_secs(2));
    if container_state(container)?.as_deref() == Some("running") {
        return Ok(());
    }
    Err(anyhow!(
        "service '{name}' exited right after start; see `docker logs {container}`"
    ))
}

fn run_dev_down() -> Result<()> {
    let config = load_or_create_config()?;
    let catalog = load_catalog()?;
    let entries = resolve_entries(&config, &catalog)?;
    for entry in entries {
        let container = container_name(&entry.name);
        match container_state(&container)?.as_deref() {
            Some("running") => {
                docker(&["stop", &container])?;
                println!("○ {} stopped", entry.name);
            }
            _ => println!("○ {} not running", entry.name),
        }
    }
    Ok(())
}

fn resolve_entries(config: &PortPilotConfig, catalog: &Catalog) -> Result<Vec<CatalogService>> {
    config
        .dev
        .services
        .iter()
        .map(|service| catalog_entry(catalog, &service.name).cloned())
        .collect()
}

fn catalog_entry<'a>(catalog: &'a Catalog, name: &str) -> Result<&'a CatalogService> {
    catalog
        .services
        .iter()
        .find(|item| item.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| anyhow!("service '{name}' was not found in the catalog"))
}

fn load_catalog() -> Result<Catalog> {
    let local = std::env::current_dir()
        .context("failed to determine current directory")?
        .join(CATALOG_FILE_NAME);
    let raw = if local.exists() {
        fs::read_to_string(&local).with_context(|| format!("failed to read {}", local.display()))?
    } else {
        DEFAULT_CATALOG.to_string()
    };
    toml::from_str(&raw).with_context(|| format!("failed to parse {CATALOG_FILE_NAME}"))
}

fn container_name(service: &str) -> String {
    format!("portpilot-{service}")
}

fn container_state(container: &str) -> Result<Option<String>> {
    let filter = format!("name=^{container}$");
    let output = std::process::Command::new("docker")
        .args(["ps", "-a", "--filter", &filter, "--format", "{{.State}}"])
        .output()
        .context("failed to run docker; is Docker installed and running?")?;
    if !output.status.success() {
        return Err(anyhow!(
            "docker ps failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let state = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!state.is_empty()).then_some(state))
}

fn docker(args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("docker")
        .args(args)
        .output()
        .context("failed to run docker; is Docker installed and running?")?;
    if !output.status.success() {
        return Err(anyhow!(
            "docker {} failed: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn print_dev_services(pilot: &PortPilot, services: &[DevService], catalog: &Catalog) {
    println!("PortPilot Dev Services\n");
    let max_len = services
        .iter()
        .map(|service| service.name.len())
        .max()
        .unwrap_or(0);

    for service in services {
        match catalog_entry(catalog, &service.name) {
            Ok(entry) => {
                let status = if pilot.find_by_port(entry.default_port).is_ok() {
                    "running"
                } else {
                    "stopped"
                };
                println!(
                    "● {:<width$} :{}   {}",
                    service.name,
                    entry.default_port,
                    status,
                    width = max_len
                );
            }
            Err(_) => println!(
                "● {:<width$} (not in catalog)",
                service.name,
                width = max_len
            ),
        }
    }
    println!();
}

fn print_dev_commands() {
    println!("Commands:");
    println!("  portpilot dev add <service>");
    println!("  portpilot dev remove <service>");
    println!("  portpilot dev up");
    println!("  portpilot dev down");
}

fn normalize_service_name(service: &str) -> Result<String> {
    let name = service.trim();
    if name.is_empty() {
        return Err(anyhow!("service name cannot be empty"));
    }
    Ok(name.to_ascii_lowercase())
}

fn config_path() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to determine current directory")?;
    Ok(cwd.join(CONFIG_FILE_NAME))
}

fn load_or_create_config() -> Result<PortPilotConfig> {
    let path = config_path()?;
    if path.exists() {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let parsed = toml::from_str::<PortPilotConfig>(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        return Ok(parsed);
    }

    let config = default_config();
    let serialized = toml::to_string_pretty(&config).context("failed to serialize config")?;
    fs::write(&path, serialized).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(config)
}

fn write_config(config: &PortPilotConfig) -> Result<()> {
    let path = config_path()?;
    let serialized = toml::to_string_pretty(config).context("failed to serialize config")?;
    fs::write(&path, serialized).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn default_config() -> PortPilotConfig {
    PortPilotConfig {
        dev: DevConfig {
            services: Vec::new(),
        },
    }
}

fn run_watch_tui(pilot: &PortPilot, interval_ms: u64) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let _terminal_guard = TerminalGuard;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut bindings = pilot.list_ports().unwrap_or_default();
    let mut table_state = TableState::default();
    if !bindings.is_empty() {
        table_state.select(Some(0));
    }
    let mut status_line =
        String::from("↑/↓ select • Enter inspect • x request kill • y confirm kill • q quit");
    let mut details_line = String::new();
    let mut pending_kill_pid: Option<u32> = None;

    let refresh_every = Duration::from_millis(interval_ms.max(100));
    let mut last_refresh = Instant::now();

    let mut should_exit = false;
    while !should_exit {
        terminal.draw(|frame| {
            let areas = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),
                    Constraint::Length(2),
                    Constraint::Length(2),
                ])
                .split(frame.area());

            let header = Row::new([
                TuiCell::from("PORT"),
                TuiCell::from("PROTO"),
                TuiCell::from("PID"),
                TuiCell::from("PROCESS"),
                TuiCell::from("CPU"),
                TuiCell::from("MEMORY"),
                TuiCell::from("STATUS"),
            ])
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );

            let rows = bindings.iter().map(|binding| {
                Row::new([
                    TuiCell::from(binding.port.to_string()),
                    TuiCell::from(format!("{:?}", binding.protocol).to_uppercase()),
                    TuiCell::from(binding.pid.to_string()),
                    TuiCell::from(binding.process.clone()),
                    TuiCell::from(format!("{:.1}%", binding.cpu_percent)),
                    TuiCell::from(format_bytes(binding.memory_bytes)),
                    TuiCell::from(binding.status.clone().unwrap_or_else(|| "-".to_string())),
                ])
            });

            let table = TuiTable::new(
                rows,
                [
                    Constraint::Length(7),
                    Constraint::Length(7),
                    Constraint::Length(8),
                    Constraint::Length(24),
                    Constraint::Length(8),
                    Constraint::Length(10),
                    Constraint::Length(12),
                ],
            )
            .header(header)
            .row_highlight_style(Style::default().bg(Color::DarkGray))
            .block(
                Block::default()
                    .title("PortPilot Watch")
                    .borders(Borders::ALL),
            );

            frame.render_stateful_widget(table, areas[0], &mut table_state);
            frame.render_widget(
                Paragraph::new(status_line.clone()).block(Block::default().borders(Borders::ALL)),
                areas[1],
            );
            frame.render_widget(
                Paragraph::new(details_line.clone()).block(Block::default().borders(Borders::ALL)),
                areas[2],
            );
        })?;

        if last_refresh.elapsed() >= refresh_every {
            bindings = pilot.list_ports().unwrap_or_default();
            normalize_selection(&mut table_state, bindings.len());
            status_line = format!("Updated at {:?}. q quit", Instant::now());
            last_refresh = Instant::now();
        }

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind == KeyEventKind::Release {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Char('Q')
                    if !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    should_exit = true
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    should_exit = true
                }
                KeyCode::Down => next_row(&mut table_state, bindings.len()),
                KeyCode::Up => prev_row(&mut table_state),
                KeyCode::Char('r') => {
                    bindings = pilot.list_ports().unwrap_or_default();
                    normalize_selection(&mut table_state, bindings.len());
                    status_line = "Refreshed".to_string();
                }
                KeyCode::Enter | KeyCode::Char('i') => {
                    if let Some(binding) = selected_binding(&bindings, &table_state) {
                        match pilot.inspect_pid(binding.pid) {
                            Ok(details) => {
                                details_line = format!(
                                    "PID {} • {} • cmd: {} • ports: {}",
                                    details.pid,
                                    details.process,
                                    details.command,
                                    details
                                        .ports
                                        .iter()
                                        .map(u16::to_string)
                                        .collect::<Vec<_>>()
                                        .join(",")
                                );
                            }
                            Err(error) => {
                                details_line = format!("Inspect failed: {error}");
                            }
                        }
                    }
                }
                KeyCode::Char('x') => {
                    if let Some(binding) = selected_binding(&bindings, &table_state) {
                        pending_kill_pid = Some(binding.pid);
                        status_line = format!(
                            "Kill requested for PID {}. Press y to confirm.",
                            binding.pid
                        );
                    }
                }
                KeyCode::Char('y') => {
                    if let Some(pid) = pending_kill_pid.take() {
                        match pilot.terminate(TerminationTarget::Pid(pid)) {
                            Ok(_) => {
                                status_line = format!("Process {pid} terminated");
                                bindings = pilot.list_ports().unwrap_or_default();
                                normalize_selection(&mut table_state, bindings.len());
                            }
                            Err(error) => {
                                status_line = format!("Kill failed for {pid}: {error}");
                            }
                        }
                    }
                }
                KeyCode::Esc => {
                    if pending_kill_pid.is_some() {
                        pending_kill_pid = None;
                        status_line = "Pending kill request cleared".to_string();
                    } else {
                        should_exit = true;
                    }
                }
                _ => {}
            }
        }
    }

    terminal.show_cursor()?;
    Ok(())
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen);
    }
}

fn selected_binding<'a>(
    bindings: &'a [PortBinding],
    state: &TableState,
) -> Option<&'a PortBinding> {
    state.selected().and_then(|idx| bindings.get(idx))
}

fn normalize_selection(state: &mut TableState, len: usize) {
    match (len, state.selected()) {
        (0, _) => state.select(None),
        (_, None) => state.select(Some(0)),
        (l, Some(i)) if i >= l => state.select(Some(l.saturating_sub(1))),
        _ => {}
    }
}

fn next_row(state: &mut TableState, len: usize) {
    if len == 0 {
        state.select(None);
        return;
    }
    let next = state.selected().map_or(0, |i| (i + 1).min(len - 1));
    state.select(Some(next));
}

fn prev_row(state: &mut TableState) {
    let prev = state.selected().map_or(0, |i| i.saturating_sub(1));
    state.select(Some(prev));
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        toml::from_str(
            r#"
            [[services]]
            name = "redis"
            image = "redis:7"
            default_port = 6379

            [[services]]
            name = "postgres"
            image = "postgres:16"
            default_port = 5432
            env = ["POSTGRES_PASSWORD=x"]
            "#,
        )
        .unwrap()
    }

    #[test]
    fn format_bytes_uses_expected_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1536 * 1024), "1.5 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn normalize_service_name_trims_and_lowercases() {
        assert_eq!(normalize_service_name("  RabbitMQ ").unwrap(), "rabbitmq");
        assert!(normalize_service_name("   ").is_err());
        assert!(normalize_service_name("").is_err());
    }

    #[test]
    fn container_name_is_prefixed() {
        assert_eq!(container_name("redis"), "portpilot-redis");
    }

    #[test]
    fn bundled_catalog_parses_and_has_unique_names() {
        let catalog: Catalog = toml::from_str(DEFAULT_CATALOG).unwrap();
        assert!(!catalog.services.is_empty());
        let mut names: Vec<_> = catalog.services.iter().map(|s| s.name.clone()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), catalog.services.len());
        assert!(catalog.services.iter().all(|s| s.default_port > 0));
    }

    #[test]
    fn catalog_entry_is_case_insensitive() {
        let catalog = catalog();
        assert_eq!(catalog_entry(&catalog, "REDIS").unwrap().image, "redis:7");
        let err = catalog_entry(&catalog, "nope").unwrap_err().to_string();
        assert!(err.contains("not found in the catalog"));
    }

    #[test]
    fn catalog_env_defaults_to_empty() {
        let catalog = catalog();
        assert!(catalog_entry(&catalog, "redis").unwrap().env.is_empty());
        assert_eq!(catalog_entry(&catalog, "postgres").unwrap().env.len(), 1);
    }

    #[test]
    fn resolve_entries_preserves_config_order_and_fails_on_unknown() {
        let catalog = catalog();
        let mut config = default_config();
        config.dev.services = vec![
            DevService {
                name: "postgres".into(),
            },
            DevService {
                name: "redis".into(),
            },
        ];
        let names: Vec<_> = resolve_entries(&config, &catalog)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["postgres", "redis"]);

        config.dev.services.push(DevService {
            name: "ghost".into(),
        });
        assert!(resolve_entries(&config, &catalog).is_err());
    }

    #[test]
    fn config_round_trips_through_toml() {
        let mut config = default_config();
        config.dev.services.push(DevService {
            name: "redis".into(),
        });
        let raw = toml::to_string_pretty(&config).unwrap();
        let parsed: PortPilotConfig = toml::from_str(&raw).unwrap();
        assert_eq!(parsed.dev.services.len(), 1);
        assert_eq!(parsed.dev.services[0].name, "redis");
    }

    #[test]
    fn row_navigation_stays_within_bounds() {
        let mut state = TableState::default();
        next_row(&mut state, 0);
        assert_eq!(state.selected(), None);

        next_row(&mut state, 3);
        assert_eq!(state.selected(), Some(0));
        next_row(&mut state, 3);
        next_row(&mut state, 3);
        next_row(&mut state, 3);
        assert_eq!(state.selected(), Some(2));

        prev_row(&mut state);
        assert_eq!(state.selected(), Some(1));
        prev_row(&mut state);
        prev_row(&mut state);
        assert_eq!(state.selected(), Some(0));
    }

    #[test]
    fn normalize_selection_clamps_and_clears() {
        let mut state = TableState::default();
        normalize_selection(&mut state, 5);
        assert_eq!(state.selected(), Some(0));

        state.select(Some(9));
        normalize_selection(&mut state, 5);
        assert_eq!(state.selected(), Some(4));

        state.select(Some(2));
        normalize_selection(&mut state, 5);
        assert_eq!(state.selected(), Some(2));

        normalize_selection(&mut state, 0);
        assert_eq!(state.selected(), None);
    }

    #[test]
    fn selected_binding_handles_none_and_out_of_range() {
        let binding = PortBinding {
            port: 1,
            protocol: portpilot::models::Protocol::Tcp,
            pid: 2,
            process: "p".into(),
            status: None,
            cpu_percent: 0.0,
            memory_bytes: 0,
        };
        let list = vec![binding];
        let mut state = TableState::default();
        assert!(selected_binding(&list, &state).is_none());
        state.select(Some(0));
        assert_eq!(selected_binding(&list, &state).unwrap().pid, 2);
        state.select(Some(5));
        assert!(selected_binding(&list, &state).is_none());
    }
}
