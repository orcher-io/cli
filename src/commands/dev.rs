//! `orcher dev`: a local engine for development, run in Docker.
//!
//! The engine is published only as a container image, so this runs that
//! image, with Postgres beside it, using the `docker` command. The containers
//! share a private network; Postgres is not published to the host, and the
//! engine's ports are bound to 127.0.0.1 only, because it runs without
//! authentication. The database lives in a named volume, so stopping the
//! engine keeps its workflows until `orcher dev stop --delete-data`.

use crate::error::{Error, Result};
use crate::settings::{GlobalArgs, Output};
use clap::{Args, Subcommand};
use console::style;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The engine version `orcher dev start` runs unless told otherwise.
pub const DEFAULT_ENGINE_VERSION: &str = "0.5.5";
const ENGINE_REPOSITORY: &str = "ghcr.io/orcher-io/orcher";
const POSTGRES_IMAGE: &str = "postgres:17-alpine";

const NETWORK: &str = "orcher-dev";
const VOLUME: &str = "orcher-dev-postgres";
const POSTGRES: &str = "orcher-dev-postgres";
const ENGINE: &str = "orcher-dev-engine";
const LABEL: &str = "io.orcher.dev=true";

/// The ports the engine listens on inside its container.
const ENGINE_GRPC_PORT: u16 = 50051;
const ENGINE_HTTP_PORT: u16 = 8080;

/// Local-only credentials: Postgres is reachable from the engine alone.
const DATABASE_URL: &str = "postgresql://orcher:orcher@orcher-dev-postgres:5432/orcher";

const POLL: Duration = Duration::from_millis(500);

#[derive(Args)]
#[command(after_help = "\
Examples:
  orcher dev start                        Run the engine on localhost:50051
  orcher dev start --grpc-port 50061      On another port
  orcher dev start --engine-version 0.5.4 A specific engine release
  orcher dev logs --follow                Follow the engine's log
  orcher dev stop --delete-data           Stop it and forget every workflow")]
pub struct DevCommand {
    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Start a local engine and wait until it is ready
    Start(StartArgs),

    /// Stop the local engine
    Stop {
        /// Also delete its database: every workflow it ran
        #[arg(long)]
        delete_data: bool,
    },

    /// Show whether the local engine is running, and where
    Status,

    /// Show the local engine's log
    Logs {
        /// Keep printing as the engine writes
        #[arg(short, long)]
        follow: bool,

        /// How many lines to show from the end
        #[arg(long, default_value = "200")]
        tail: String,

        /// Show the database's log instead of the engine's
        #[arg(long)]
        postgres: bool,
    },
}

#[derive(Args, Clone)]
struct StartArgs {
    /// Engine release to run
    #[arg(long, env = "ORCHER_ENGINE_VERSION", default_value = DEFAULT_ENGINE_VERSION)]
    engine_version: String,

    /// Engine image to run, overriding --engine-version (for a mirror, say)
    #[arg(long, env = "ORCHER_ENGINE_IMAGE")]
    image: Option<String>,

    /// Host port for gRPC: workers and the CLI connect here
    #[arg(long, default_value_t = ENGINE_GRPC_PORT)]
    grpc_port: u16,

    /// Host port for HTTP: /health/ready and /metrics
    #[arg(long, default_value_t = ENGINE_HTTP_PORT)]
    http_port: u16,

    /// Engine log level: trace, debug, info, warn, error
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Seconds to wait for the engine to become ready
    #[arg(long, default_value_t = 120)]
    timeout: u64,
}

impl StartArgs {
    fn image(&self) -> String {
        self.image
            .clone()
            .unwrap_or_else(|| format!("{ENGINE_REPOSITORY}:{}", self.engine_version))
    }
}

pub async fn run(cmd: DevCommand, args: &GlobalArgs) -> Result<()> {
    docker_available()?;
    match cmd.action {
        Action::Start(start_args) => start(&start_args, args).await,
        Action::Stop { delete_data } => stop(delete_data, args),
        Action::Status => status(args),
        Action::Logs {
            follow,
            tail,
            postgres,
        } => logs(follow, &tail, postgres),
    }
}

async fn start(start: &StartArgs, args: &GlobalArgs) -> Result<()> {
    let image = start.image();
    let say = |text: String| {
        if !args.quiet && !args.is_structured() {
            eprintln!("{text}");
        }
    };

    // An engine that is already up is fine if it is the one asked for.
    if let Some(engine) = inspect(ENGINE)? {
        if engine.running {
            if engine.image == image
                && engine.grpc_port == Some(start.grpc_port)
                && engine.http_port == Some(start.http_port)
            {
                say(format!("The local engine is already running ({image})."));
                return wait_until_ready(start, args).await;
            }
            return Err(Error::local(format!(
                "a local engine is already running {} with gRPC on port {}.\n  \
                 Stop it first with `orcher dev stop` to start {image} on port {}.",
                engine.image,
                engine.grpc_port.map_or("?".to_string(), |p| p.to_string()),
                start.grpc_port,
            )));
        }
        // Stopped or failed: replace it. Its data is in the volume.
        docker(&["rm", "-f", ENGINE])?;
    }

    if !image_present(&image)? {
        say(format!("Pulling {image}…"));
        pull(&image, args.quiet || args.is_structured())?;
    }

    ensure_network()?;
    ensure_volume()?;
    ensure_postgres(args)?;
    wait_healthy(POSTGRES, Duration::from_secs(60)).await?;

    say(format!("Starting {image}…"));
    let run_args = engine_run_args(start, &image);
    let run_args: Vec<&str> = run_args.iter().map(String::as_str).collect();
    if let Err(e) = docker(&run_args) {
        // `docker run` leaves a created container behind when it cannot start
        // it; remove it so that the next start begins clean.
        let _ = docker(&["rm", "-f", ENGINE]);
        if port_in_use(&e) {
            return Err(Error::local(format!(
                "port {} or {} is already in use on this machine.\n  \
                 Pick others: orcher dev start --grpc-port 50061 --http-port 8081",
                start.grpc_port, start.http_port
            )));
        }
        return Err(e);
    }

    wait_until_ready(start, args).await
}

async fn wait_until_ready(start: &StartArgs, args: &GlobalArgs) -> Result<()> {
    if let Err(err) = wait_healthy(ENGINE, Duration::from_secs(start.timeout)).await {
        let tail = docker(&["logs", "--tail", "30", ENGINE]).unwrap_or_default();
        return Err(Error::local(format!(
            "{err}\n  The engine's last log lines:\n{tail}\n  More with `orcher dev logs`."
        )));
    }
    if args.quiet {
        return Ok(());
    }
    let engine = inspect(ENGINE)?.ok_or_else(|| Error::local("the engine container vanished"))?;
    print_status(Some(&engine), inspect(POSTGRES)?.as_ref(), args)?;
    if !args.is_structured() {
        if start.grpc_port != ENGINE_GRPC_PORT {
            eprintln!(
                "\nPoint the CLI and your workers at it:\n  export ORCHER_SERVER=http://localhost:{}",
                start.grpc_port
            );
        }
        eprintln!(
            "\n{}",
            style("The engine image is a free developer preview, for development and evaluation.")
                .dim()
        );
    }
    Ok(())
}

fn stop(delete_data: bool, args: &GlobalArgs) -> Result<()> {
    let mut removed = false;
    for name in [ENGINE, POSTGRES] {
        if inspect(name)?.is_some() {
            docker(&["rm", "-f", name])?;
            removed = true;
        }
    }
    if network_exists()? {
        docker(&["network", "rm", NETWORK])?;
    }
    let mut deleted = false;
    if delete_data && volume_exists()? {
        docker(&["volume", "rm", VOLUME])?;
        deleted = true;
    }
    if !args.quiet {
        match (removed, deleted) {
            (true, true) => println!("Local engine stopped and its data deleted."),
            (true, false) => println!("Local engine stopped. Its data is kept for the next start."),
            (false, true) => println!("No local engine was running. Its data is deleted."),
            (false, false) => println!("No local engine is running."),
        }
    }
    Ok(())
}

fn status(args: &GlobalArgs) -> Result<()> {
    print_status(inspect(ENGINE)?.as_ref(), inspect(POSTGRES)?.as_ref(), args)
}

fn print_status(
    engine: Option<&Container>,
    postgres: Option<&Container>,
    args: &GlobalArgs,
) -> Result<()> {
    let value = serde_json::json!({
        "engine": engine.map(|e| serde_json::json!({
            "state": e.state_word(),
            "image": e.image,
            "grpcPort": e.grpc_port,
            "httpPort": e.http_port,
        })),
        "postgres": postgres.map(|p| serde_json::json!({
            "state": p.state_word(),
            "image": p.image,
        })),
    });
    if crate::render::structured(args, &value)? {
        return Ok(());
    }
    if args.output == Output::Name {
        if engine.is_some_and(|e| e.running) {
            println!("{ENGINE}");
        }
        return Ok(());
    }
    let Some(engine) = engine else {
        println!("The local engine is not running. Start it with `orcher dev start`.");
        return Ok(());
    };
    let field = |name: &str, value: String| println!("  {:<10}{}", style(name).dim(), value);
    println!("{}", style("Local engine").bold());
    field(
        "State",
        crate::render::status_cell_str(&engine.state_word()),
    );
    field("Image", engine.image.clone());
    if let Some(port) = engine.grpc_port {
        field("gRPC", format!("http://localhost:{port}"));
    }
    if let Some(port) = engine.http_port {
        field("Health", format!("http://localhost:{port}/health/ready"));
    }
    field(
        "Database",
        postgres.map_or("missing".to_string(), Container::state_word),
    );
    Ok(())
}

fn logs(follow: bool, tail: &str, postgres: bool) -> Result<()> {
    let name = if postgres { POSTGRES } else { ENGINE };
    if inspect(name)?.is_none() {
        return Err(Error::local(
            "the local engine is not running. Start it with `orcher dev start`.",
        ));
    }
    let mut command = Command::new("docker");
    command.args(["logs", "--tail", tail]);
    if follow {
        command.arg("--follow");
    }
    let status = command.arg(name).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::local(format!("docker logs exited with {status}")))
    }
}

/// The `docker run` arguments for the engine container.
fn engine_run_args(start: &StartArgs, image: &str) -> Vec<String> {
    let mut a: Vec<String> = [
        "run",
        "--detach",
        "--name",
        ENGINE,
        "--label",
        LABEL,
        "--network",
        NETWORK,
        "--restart",
        "unless-stopped",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let env = [
        format!("DATABASE_URL={DATABASE_URL}"),
        format!("GRPC_BIND_ADDR=0.0.0.0:{ENGINE_GRPC_PORT}"),
        format!("HTTP_BIND_ADDR=0.0.0.0:{ENGINE_HTTP_PORT}"),
        format!("LOG_LEVEL={}", start.log_level),
        "LOG_FORMAT=compact".to_string(),
    ];
    for e in env {
        a.push("--env".into());
        a.push(e);
    }
    a.push("--publish".into());
    a.push(format!("127.0.0.1:{}:{ENGINE_GRPC_PORT}", start.grpc_port));
    a.push("--publish".into());
    a.push(format!("127.0.0.1:{}:{ENGINE_HTTP_PORT}", start.http_port));
    // The image's own health check, polled often so that start returns as
    // soon as the engine has migrated its database and answers.
    for h in [
        "--health-cmd=orcher-entrypoint healthcheck",
        "--health-interval=2s",
        "--health-timeout=3s",
        "--health-retries=30",
        "--health-start-period=60s",
    ] {
        a.push(h.to_string());
    }
    a.push(image.to_string());
    a
}

fn ensure_postgres(args: &GlobalArgs) -> Result<()> {
    match inspect(POSTGRES)? {
        Some(c) if c.running => Ok(()),
        Some(_) => docker(&["start", POSTGRES]).map(|_| ()),
        None => {
            if !image_present(POSTGRES_IMAGE)? {
                pull(POSTGRES_IMAGE, args.quiet || args.is_structured())?;
            }
            docker(&[
                "run",
                "--detach",
                "--name",
                POSTGRES,
                "--label",
                LABEL,
                "--network",
                NETWORK,
                "--restart",
                "unless-stopped",
                "--env",
                "POSTGRES_USER=orcher",
                "--env",
                "POSTGRES_PASSWORD=orcher",
                "--env",
                "POSTGRES_DB=orcher",
                "--volume",
                &format!("{VOLUME}:/var/lib/postgresql/data"),
                "--health-cmd=pg_isready -U orcher -d orcher",
                "--health-interval=1s",
                "--health-timeout=3s",
                "--health-retries=60",
                POSTGRES_IMAGE,
            ])
            .map(|_| ())
        }
    }
}

fn ensure_network() -> Result<()> {
    if !network_exists()? {
        docker(&["network", "create", "--label", LABEL, NETWORK])?;
    }
    Ok(())
}

fn ensure_volume() -> Result<()> {
    if !volume_exists()? {
        docker(&["volume", "create", "--label", LABEL, VOLUME])?;
    }
    Ok(())
}

fn network_exists() -> Result<bool> {
    succeeds(&["network", "inspect", NETWORK])
}

fn volume_exists() -> Result<bool> {
    succeeds(&["volume", "inspect", VOLUME])
}

fn image_present(image: &str) -> Result<bool> {
    succeeds(&["image", "inspect", image])
}

fn pull(image: &str, quiet: bool) -> Result<()> {
    let mut command = Command::new("docker");
    command.args(["pull", image]);
    if quiet {
        command.arg("--quiet").stdout(Stdio::null());
    } else {
        // Progress goes to stderr, keeping stdout for the command's output.
        command.stdout(std::io::stderr());
    }
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::local(format!(
            "could not pull {image}. Check the version exists and that this machine can reach the registry."
        )))
    }
}

/// Waits until a container's health check passes.
async fn wait_healthy(name: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let container = inspect(name)?
            .ok_or_else(|| Error::local(format!("container {name} disappeared while starting")))?;
        match (container.running, container.health.as_deref()) {
            (true, Some("healthy")) => return Ok(()),
            (false, _) => {
                return Err(Error::local(format!(
                    "{name} stopped while starting (state: {})",
                    container.state
                )))
            }
            _ => {}
        }
        if Instant::now() >= deadline {
            return Err(Error::local(format!(
                "{name} was not ready after {}s (health: {})",
                timeout.as_secs(),
                container.health.as_deref().unwrap_or("none")
            )));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// What `docker inspect` says about one of our containers.
#[derive(Debug, PartialEq)]
struct Container {
    state: String,
    running: bool,
    health: Option<String>,
    image: String,
    grpc_port: Option<u16>,
    http_port: Option<u16>,
}

impl Container {
    fn from_inspect(value: &serde_json::Value) -> Self {
        let state = &value["State"];
        let host_port = |container_port: u16| {
            value["HostConfig"]["PortBindings"][format!("{container_port}/tcp")][0]["HostPort"]
                .as_str()
                .and_then(|p| p.parse().ok())
        };
        Self {
            state: state["Status"].as_str().unwrap_or("unknown").to_string(),
            running: state["Running"].as_bool().unwrap_or(false),
            health: state["Health"]["Status"].as_str().map(str::to_string),
            image: value["Config"]["Image"].as_str().unwrap_or("").to_string(),
            grpc_port: host_port(ENGINE_GRPC_PORT),
            http_port: host_port(ENGINE_HTTP_PORT),
        }
    }

    /// "ready", "starting", "unhealthy", or Docker's state when stopped.
    fn state_word(&self) -> String {
        match (self.running, self.health.as_deref()) {
            (true, Some("healthy")) => "ready".to_string(),
            (true, Some("starting")) => "starting".to_string(),
            (true, Some(other)) => other.to_string(),
            (true, None) => "running".to_string(),
            (false, _) => self.state.clone(),
        }
    }
}

fn inspect(name: &str) -> Result<Option<Container>> {
    let output = Command::new("docker")
        .args(["container", "inspect", name])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    Ok(parsed.get(0).map(Container::from_inspect))
}

/// Runs `docker` and returns its stdout, or its stderr as the error.
fn docker(args: &[&str]) -> Result<String> {
    let output = Command::new("docker").args(args).output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(Error::local(format!(
            "docker {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn succeeds(args: &[&str]) -> Result<bool> {
    Ok(Command::new("docker")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success())
}

fn port_in_use(err: &Error) -> bool {
    let text = err.to_string();
    text.contains("port is already allocated") || text.contains("address already in use")
}

/// Fails with a clear message unless Docker is installed and its daemon
/// answers.
fn docker_available() -> Result<()> {
    let output = match Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
    {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::local(
                "Docker is not installed, or `docker` is not on your PATH.\n  \
                 `orcher dev` runs the engine in Docker: install it from https://docs.docker.com/get-docker/",
            ))
        }
        Err(e) => return Err(Error::local(format!("cannot run docker: {e}"))),
    };
    if output.status.success() {
        return Ok(());
    }
    let reason = String::from_utf8_lossy(&output.stderr);
    let reason = reason.lines().last().unwrap_or("").trim();
    Err(Error::local(format!(
        "Docker is installed but not answering; is it running?\n  {reason}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start_args() -> StartArgs {
        StartArgs {
            engine_version: DEFAULT_ENGINE_VERSION.to_string(),
            image: None,
            grpc_port: 50061,
            http_port: 8081,
            log_level: "debug".to_string(),
            timeout: 120,
        }
    }

    #[test]
    fn the_image_follows_the_version_unless_overridden() {
        let mut a = start_args();
        assert_eq!(a.image(), "ghcr.io/orcher-io/orcher:0.5.5");
        a.engine_version = "0.5.4".to_string();
        assert_eq!(a.image(), "ghcr.io/orcher-io/orcher:0.5.4");
        a.image = Some("mirror.example.com/orcher:0.5.4".to_string());
        assert_eq!(a.image(), "mirror.example.com/orcher:0.5.4");
    }

    #[test]
    fn the_engine_is_published_on_localhost_only() {
        let args = engine_run_args(&start_args(), "img:1");
        let joined = args.join(" ");
        assert!(
            joined.contains("--publish 127.0.0.1:50061:50051"),
            "{joined}"
        );
        assert!(joined.contains("--publish 127.0.0.1:8081:8080"), "{joined}");
        assert!(joined.contains("--env LOG_LEVEL=debug"), "{joined}");
        assert!(joined.contains("--network orcher-dev"), "{joined}");
        assert_eq!(args.last().map(String::as_str), Some("img:1"));
    }

    #[test]
    fn inspect_output_is_read() {
        let value = serde_json::json!({
            "State": {"Status": "running", "Running": true, "Health": {"Status": "healthy"}},
            "Config": {"Image": "ghcr.io/orcher-io/orcher:0.5.5"},
            "HostConfig": {"PortBindings": {
                "50051/tcp": [{"HostIp": "127.0.0.1", "HostPort": "50061"}],
                "8080/tcp": [{"HostIp": "127.0.0.1", "HostPort": "8081"}]
            }}
        });
        let c = Container::from_inspect(&value);
        assert!(c.running);
        assert_eq!(c.state_word(), "ready");
        assert_eq!(c.grpc_port, Some(50061));
        assert_eq!(c.http_port, Some(8081));
        assert_eq!(c.image, "ghcr.io/orcher-io/orcher:0.5.5");

        let stopped = Container::from_inspect(&serde_json::json!({
            "State": {"Status": "exited", "Running": false},
            "Config": {"Image": "postgres:17-alpine"},
            "HostConfig": {"PortBindings": {}}
        }));
        assert_eq!(stopped.state_word(), "exited");
        assert_eq!(stopped.grpc_port, None);
    }

    #[test]
    fn port_conflicts_are_recognized() {
        let err = Error::local(
            "docker run failed: Bind for 127.0.0.1:50051 failed: port is already allocated",
        );
        assert!(port_in_use(&err));
        assert!(!port_in_use(&Error::local(
            "docker run failed: no such image"
        )));
    }
}
