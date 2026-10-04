//! A local engine run in Docker, shared by `orcher dev` and `orcher server`.
//!
//! The engine is published only as a container image, so the CLI runs that
//! image with the `docker` command, with Postgres beside it unless a database
//! URL is given. The containers share a private network; Postgres is not
//! published to the host, and the engine's ports are bound to 127.0.0.1 by
//! default, because the image runs without authentication unless a config
//! file turns it on. The database lives in a named volume, so stopping the
//! engine keeps its workflows until the data is deleted on purpose.

use crate::error::{CliError, Result};
use console::style;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The engine release run unless another is asked for.
pub const DEFAULT_ENGINE_VERSION: &str = "0.5.5";
pub const ENGINE_REPOSITORY: &str = "ghcr.io/orcher-io/orcher";
const POSTGRES_IMAGE: &str = "postgres:17-alpine";

pub const NETWORK: &str = "orcher-dev";
pub const VOLUME: &str = "orcher-dev-postgres";
pub const POSTGRES: &str = "orcher-dev-postgres";
pub const ENGINE: &str = "orcher-dev-engine";
const LABEL: &str = "io.orcher.dev=true";

/// The ports the engine listens on inside its container.
pub const ENGINE_GRPC_PORT: u16 = 50051;
pub const ENGINE_HTTP_PORT: u16 = 8080;

/// Where a config file given with `--config` is mounted in the container.
const CONFIG_MOUNT: &str = "/etc/orcher/orchestrator.toml";

/// Local-only credentials: Postgres is reachable from the engine alone.
const LOCAL_DATABASE_URL: &str = "postgresql://orcher:orcher@orcher-dev-postgres:5432/orcher";

const POLL: Duration = Duration::from_millis(500);

/// How to run the engine.
#[derive(Debug, Clone)]
pub struct EngineOptions {
    /// Image reference, such as `ghcr.io/orcher-io/orcher:0.5.5`.
    pub image: String,
    /// Host port published for gRPC.
    pub grpc_port: u16,
    /// Host port published for HTTP health and metrics.
    pub http_port: u16,
    /// Host address the ports are published on.
    pub bind_host: String,
    /// Engine log level.
    pub log_level: String,
    /// How long to wait for the engine to become ready.
    pub timeout: Duration,
    /// An existing database to use instead of a Postgres container.
    pub database_url: Option<String>,
    /// An engine config file (TOML), mounted read-only.
    pub config_file: Option<PathBuf>,
}

impl EngineOptions {
    pub fn image_for(version: &str, image: Option<&str>) -> String {
        image
            .map(str::to_string)
            .unwrap_or_else(|| format!("{ENGINE_REPOSITORY}:{version}"))
    }
}

/// Starts the engine, or confirms that the same one is already running, and
/// waits until it is ready.
pub async fn start(opts: &EngineOptions, quiet: bool) -> Result<()> {
    docker_available()?;
    let say = |text: String| {
        if !quiet {
            eprintln!("{text}");
        }
    };

    if let Some(engine) = inspect(ENGINE)? {
        if engine.running {
            if engine.image == opts.image
                && engine.grpc_port == Some(opts.grpc_port)
                && engine.http_port == Some(opts.http_port)
            {
                say(format!(
                    "The local engine is already running ({}).",
                    opts.image
                ));
                return wait_healthy_or_explain(opts.timeout).await;
            }
            return Err(CliError::local_engine(format!(
                "A local engine is already running {} with gRPC on port {}.\n  \
                 Stop it first with `orcher server stop` to start {} on port {}.",
                engine.image,
                engine.grpc_port.map_or("?".to_string(), |p| p.to_string()),
                opts.image,
                opts.grpc_port,
            )));
        }
        // Stopped or failed: replace it. Its data is in the volume.
        docker(&["rm", "-f", ENGINE])?;
    }

    if !image_present(&opts.image)? {
        say(format!("Pulling {}…", opts.image));
        pull(&opts.image, quiet)?;
    }

    ensure_network()?;
    if opts.database_url.is_none() {
        ensure_volume()?;
        ensure_postgres(quiet)?;
        wait_healthy(POSTGRES, Duration::from_secs(60)).await?;
    }

    say(format!("Starting {}…", opts.image));
    let run_args = engine_run_args(opts)?;
    let run_args: Vec<&str> = run_args.iter().map(String::as_str).collect();
    if let Err(e) = docker(&run_args) {
        // `docker run` leaves a created container behind when it cannot start
        // it; remove it so that the next start begins clean.
        let _ = docker(&["rm", "-f", ENGINE]);
        if port_in_use(&e) {
            return Err(CliError::local_engine(format!(
                "Port {} or {} is already in use on this machine.\n  \
                 Pick others, for example --grpc-port 50061 --http-port 8081.",
                opts.grpc_port, opts.http_port
            )));
        }
        return Err(e);
    }

    wait_healthy_or_explain(opts.timeout).await
}

async fn wait_healthy_or_explain(timeout: Duration) -> Result<()> {
    if let Err(err) = wait_healthy(ENGINE, timeout).await {
        let tail = docker(&["logs", "--tail", "30", ENGINE]).unwrap_or_default();
        return Err(CliError::local_engine(format!(
            "{err}\n  The engine's last log lines:\n{tail}\n  More with `orcher dev logs`."
        )));
    }
    Ok(())
}

/// Stops the engine and its database. Returns whether anything was running.
/// `graceful` gives the engine `timeout` to shut down instead of killing it.
pub fn stop(delete_data: bool, graceful: Option<Duration>) -> Result<StopOutcome> {
    docker_available()?;
    let mut removed = false;
    for name in [ENGINE, POSTGRES] {
        if inspect(name)?.is_some() {
            if let Some(timeout) = graceful {
                let secs = timeout.as_secs().to_string();
                let _ = docker(&["stop", "--time", &secs, name]);
            }
            docker(&["rm", "-f", name])?;
            removed = true;
        }
    }
    if succeeds(&["network", "inspect", NETWORK])? {
        docker(&["network", "rm", NETWORK])?;
    }
    let mut deleted = false;
    if delete_data && succeeds(&["volume", "inspect", VOLUME])? {
        docker(&["volume", "rm", VOLUME])?;
        deleted = true;
    }
    Ok(StopOutcome { removed, deleted })
}

pub struct StopOutcome {
    pub removed: bool,
    pub deleted: bool,
}

impl StopOutcome {
    pub fn message(&self) -> &'static str {
        match (self.removed, self.deleted) {
            (true, true) => "Local engine stopped and its data deleted.",
            (true, false) => "Local engine stopped. Its data is kept for the next start.",
            (false, true) => "No local engine was running. Its data is deleted.",
            (false, false) => "No local engine is running.",
        }
    }
}

/// Prints the engine's or the database's log, following it if asked.
pub fn logs(follow: bool, tail: &str, postgres: bool) -> Result<()> {
    docker_available()?;
    let name = if postgres { POSTGRES } else { ENGINE };
    if inspect(name)?.is_none() {
        return Err(CliError::local_engine(
            "The local engine is not running. Start it with `orcher dev start`.",
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
        Err(CliError::local_engine(format!(
            "docker logs exited with {status}"
        )))
    }
}

/// The options the running engine was started with, read back from its
/// container, so that it can be restarted as it was.
pub fn running_options() -> Result<Option<EngineOptions>> {
    docker_available()?;
    let output = Command::new("docker")
        .args(["container", "inspect", ENGINE])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    Ok(parsed.get(0).map(options_from_inspect))
}

fn options_from_inspect(value: &serde_json::Value) -> EngineOptions {
    let container = Container::from_inspect(value);
    let env = |name: &str| {
        value["Config"]["Env"].as_array().and_then(|vars| {
            vars.iter()
                .filter_map(|v| v.as_str())
                .find_map(|v| v.strip_prefix(&format!("{name}=")).map(str::to_string))
        })
    };
    let bind_host = value["HostConfig"]["PortBindings"][format!("{ENGINE_GRPC_PORT}/tcp")][0]
        ["HostIp"]
        .as_str()
        .filter(|h| !h.is_empty())
        .unwrap_or("127.0.0.1")
        .to_string();
    let config_file = value["Mounts"].as_array().and_then(|mounts| {
        mounts
            .iter()
            .find(|m| m["Destination"] == CONFIG_MOUNT)
            .and_then(|m| m["Source"].as_str())
            .map(PathBuf::from)
    });
    EngineOptions {
        image: container.image,
        grpc_port: container.grpc_port.unwrap_or(ENGINE_GRPC_PORT),
        http_port: container.http_port.unwrap_or(ENGINE_HTTP_PORT),
        bind_host,
        log_level: env("LOG_LEVEL").unwrap_or_else(|| "info".to_string()),
        timeout: Duration::from_secs(120),
        database_url: env("DATABASE_URL").filter(|url| url != LOCAL_DATABASE_URL),
        config_file,
    }
}

/// The engine's and the database's containers, as `docker inspect` sees them.
pub fn status() -> Result<(Option<Container>, Option<Container>)> {
    docker_available()?;
    Ok((inspect(ENGINE)?, inspect(POSTGRES)?))
}

/// The local engine's state as JSON, for `-o json`.
pub fn status_json(engine: Option<&Container>, postgres: Option<&Container>) -> serde_json::Value {
    serde_json::json!({
        "engine": engine.map(|e| serde_json::json!({
            "state": e.state_word(),
            "image": e.image,
            "version": e.version,
            "grpcPort": e.grpc_port,
            "httpPort": e.http_port,
        })),
        "postgres": postgres.map(|p| serde_json::json!({
            "state": p.state_word(),
            "image": p.image,
        })),
    })
}

/// Prints the local engine's state for people.
pub fn print_status(engine: Option<&Container>, postgres: Option<&Container>) {
    let Some(engine) = engine else {
        println!("The local engine is not running. Start it with `orcher dev start`.");
        return;
    };
    let field = |name: &str, value: String| println!("  {:<10}{}", style(name).dim(), value);
    println!("{}", style("Local engine").bold());
    field("State", engine.state_word());
    field("Image", engine.image.clone());
    if let Some(port) = engine.grpc_port {
        field("gRPC", format!("http://localhost:{port}"));
    }
    if let Some(port) = engine.http_port {
        field("Health", format!("http://localhost:{port}/health/ready"));
    }
    field(
        "Database",
        postgres.map_or("external".to_string(), Container::state_word),
    );
}

/// What to say after a start: where the engine is and what it is.
pub fn print_started(opts: &EngineOptions) {
    if opts.grpc_port != ENGINE_GRPC_PORT {
        eprintln!(
            "\nPoint the CLI and your workers at it:\n  export ORCHER_SERVER=http://localhost:{}",
            opts.grpc_port
        );
    }
    eprintln!(
        "\n{}",
        style("The engine image is a free developer preview, for development and evaluation.")
            .dim()
    );
}

/// The `docker run` arguments for the engine container.
fn engine_run_args(opts: &EngineOptions) -> Result<Vec<String>> {
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

    let database_url = match &opts.database_url {
        Some(url) => {
            // The engine runs in a container, where localhost is the container
            // itself; a database on this machine is reached via the host.
            a.push("--add-host=host.docker.internal:host-gateway".into());
            container_reachable(url)
        }
        None => LOCAL_DATABASE_URL.to_string(),
    };
    let mut env = vec![
        format!("DATABASE_URL={database_url}"),
        format!("GRPC_BIND_ADDR=0.0.0.0:{ENGINE_GRPC_PORT}"),
        format!("HTTP_BIND_ADDR=0.0.0.0:{ENGINE_HTTP_PORT}"),
        format!("LOG_LEVEL={}", opts.log_level),
        "LOG_FORMAT=compact".to_string(),
    ];
    if let Some(file) = &opts.config_file {
        let file = absolute(file)?;
        a.push("--volume".into());
        a.push(format!("{}:{CONFIG_MOUNT}:ro", file.display()));
        env.push(format!("ORCHER_ORCHESTRATOR_CONFIG={CONFIG_MOUNT}"));
    }
    for e in env {
        a.push("--env".into());
        a.push(e);
    }
    a.push("--publish".into());
    a.push(format!(
        "{}:{}:{ENGINE_GRPC_PORT}",
        opts.bind_host, opts.grpc_port
    ));
    a.push("--publish".into());
    a.push(format!(
        "{}:{}:{ENGINE_HTTP_PORT}",
        opts.bind_host, opts.http_port
    ));
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
    a.push(opts.image.clone());
    Ok(a)
}

/// Rewrites a database URL naming this machine so that it works from inside
/// the engine's container.
fn container_reachable(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) if matches!(parsed.host_str(), Some("localhost" | "127.0.0.1")) => {
            let _ = parsed.set_host(Some("host.docker.internal"));
            parsed.to_string()
        }
        _ => url.to_string(),
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    let path = std::fs::canonicalize(path).map_err(|e| {
        CliError::local_engine(format!("Cannot read config file {}: {e}", path.display()))
    })?;
    Ok(path)
}

fn ensure_postgres(quiet: bool) -> Result<()> {
    match inspect(POSTGRES)? {
        Some(c) if c.running => Ok(()),
        Some(_) => docker(&["start", POSTGRES]).map(|_| ()),
        None => {
            if !image_present(POSTGRES_IMAGE)? {
                pull(POSTGRES_IMAGE, quiet)?;
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
    if !succeeds(&["network", "inspect", NETWORK])? {
        docker(&["network", "create", "--label", LABEL, NETWORK])?;
    }
    Ok(())
}

fn ensure_volume() -> Result<()> {
    if !succeeds(&["volume", "inspect", VOLUME])? {
        docker(&["volume", "create", "--label", LABEL, VOLUME])?;
    }
    Ok(())
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
    if command.status()?.success() {
        Ok(())
    } else {
        Err(CliError::local_engine(format!(
            "Could not pull {image}. Check that the version exists and that this machine can reach the registry."
        )))
    }
}

/// Waits until a container's health check passes.
async fn wait_healthy(name: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let container = inspect(name)?.ok_or_else(|| {
            CliError::local_engine(format!("Container {name} disappeared while starting"))
        })?;
        match (container.running, container.health.as_deref()) {
            (true, Some("healthy")) => return Ok(()),
            (false, _) => {
                return Err(CliError::local_engine(format!(
                    "{name} stopped while starting (state: {})",
                    container.state
                )))
            }
            _ => {}
        }
        if Instant::now() >= deadline {
            return Err(CliError::local_engine(format!(
                "{name} was not ready after {}s (health: {})",
                timeout.as_secs(),
                container.health.as_deref().unwrap_or("none")
            )));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// What `docker inspect` says about one of the local engine's containers.
#[derive(Debug, PartialEq)]
pub struct Container {
    pub state: String,
    pub running: bool,
    pub health: Option<String>,
    pub image: String,
    /// The image's `org.opencontainers.image.version` label.
    pub version: Option<String>,
    pub grpc_port: Option<u16>,
    pub http_port: Option<u16>,
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
            version: value["Config"]["Labels"]["org.opencontainers.image.version"]
                .as_str()
                .map(str::to_string),
            grpc_port: host_port(ENGINE_GRPC_PORT),
            http_port: host_port(ENGINE_HTTP_PORT),
        }
    }

    /// "ready", "starting", "unhealthy", or Docker's state when stopped.
    pub fn state_word(&self) -> String {
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
        Err(CliError::local_engine(format!(
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

fn port_in_use(err: &CliError) -> bool {
    let text = err.to_string();
    text.contains("port is already allocated") || text.contains("address already in use")
}

/// Fails with a clear message unless Docker is installed and its daemon
/// answers.
pub fn docker_available() -> Result<()> {
    let output = match Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
    {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CliError::local_engine(
                "Docker is not installed, or `docker` is not on your PATH.\n  \
                 The local engine runs in Docker: install it from https://docs.docker.com/get-docker/",
            ))
        }
        Err(e) => return Err(CliError::local_engine(format!("Cannot run docker: {e}"))),
    };
    if output.status.success() {
        return Ok(());
    }
    let reason = String::from_utf8_lossy(&output.stderr);
    let reason = reason.lines().last().unwrap_or("").trim();
    Err(CliError::local_engine(format!(
        "Docker is installed but not answering; is it running?\n  {reason}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> EngineOptions {
        EngineOptions {
            image: "img:1".to_string(),
            grpc_port: 50061,
            http_port: 8081,
            bind_host: "127.0.0.1".to_string(),
            log_level: "debug".to_string(),
            timeout: Duration::from_secs(120),
            database_url: None,
            config_file: None,
        }
    }

    #[test]
    fn the_image_follows_the_version_unless_overridden() {
        assert_eq!(
            EngineOptions::image_for("0.5.5", None),
            "ghcr.io/orcher-io/orcher:0.5.5"
        );
        assert_eq!(
            EngineOptions::image_for("0.5.5", Some("mirror.example.com/orcher:0.5.4")),
            "mirror.example.com/orcher:0.5.4"
        );
    }

    #[test]
    fn the_engine_is_published_on_the_bind_host_only() {
        let args = engine_run_args(&options()).unwrap();
        let joined = args.join(" ");
        assert!(
            joined.contains("--publish 127.0.0.1:50061:50051"),
            "{joined}"
        );
        assert!(joined.contains("--publish 127.0.0.1:8081:8080"), "{joined}");
        assert!(joined.contains("--env LOG_LEVEL=debug"), "{joined}");
        assert!(joined.contains(LOCAL_DATABASE_URL), "{joined}");
        assert!(!joined.contains("ORCHER_ORCHESTRATOR_CONFIG"), "{joined}");
        assert_eq!(args.last().map(String::as_str), Some("img:1"));
    }

    #[test]
    fn a_config_file_is_mounted_and_named() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut opts = options();
        opts.config_file = Some(file.path().to_path_buf());
        let joined = engine_run_args(&opts).unwrap().join(" ");
        assert!(joined.contains(&format!("{CONFIG_MOUNT}:ro")), "{joined}");
        assert!(
            joined.contains(&format!("ORCHER_ORCHESTRATOR_CONFIG={CONFIG_MOUNT}")),
            "{joined}"
        );

        opts.config_file = Some(PathBuf::from("/no/such/file.toml"));
        assert!(engine_run_args(&opts).is_err());
    }

    #[test]
    fn a_database_on_this_machine_is_reached_through_the_host() {
        assert_eq!(
            container_reachable("postgresql://u:p@localhost:5432/orcher"),
            "postgresql://u:p@host.docker.internal:5432/orcher"
        );
        assert_eq!(
            container_reachable("postgresql://u:p@db.internal:5432/orcher"),
            "postgresql://u:p@db.internal:5432/orcher"
        );
        let mut opts = options();
        opts.database_url = Some("postgresql://u:p@127.0.0.1:5432/orcher".to_string());
        let joined = engine_run_args(&opts).unwrap().join(" ");
        assert!(
            joined.contains("host.docker.internal:host-gateway"),
            "{joined}"
        );
        assert!(joined.contains("@host.docker.internal:5432"), "{joined}");
    }

    #[test]
    fn inspect_output_is_read() {
        let value = serde_json::json!({
            "State": {"Status": "running", "Running": true, "Health": {"Status": "healthy"}},
            "Config": {"Image": "ghcr.io/orcher-io/orcher:0.5.5",
                       "Labels": {"org.opencontainers.image.version": "0.5.5"}},
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
        assert_eq!(c.version.as_deref(), Some("0.5.5"));

        let stopped = Container::from_inspect(&serde_json::json!({
            "State": {"Status": "exited", "Running": false},
            "Config": {"Image": "postgres:17-alpine"},
            "HostConfig": {"PortBindings": {}}
        }));
        assert_eq!(stopped.state_word(), "exited");
        assert_eq!(stopped.grpc_port, None);
    }

    #[test]
    fn a_running_engine_restarts_with_its_own_settings() {
        let value = serde_json::json!({
            "State": {"Status": "running", "Running": true},
            "Config": {"Image": "ghcr.io/orcher-io/orcher:0.5.4",
                       "Env": ["LOG_LEVEL=debug", format!("DATABASE_URL={LOCAL_DATABASE_URL}")]},
            "HostConfig": {"PortBindings": {
                "50051/tcp": [{"HostIp": "0.0.0.0", "HostPort": "50061"}],
                "8080/tcp": [{"HostIp": "0.0.0.0", "HostPort": "8081"}]
            }},
            "Mounts": [{"Source": "/home/me/auth.toml", "Destination": CONFIG_MOUNT}]
        });
        let opts = options_from_inspect(&value);
        assert_eq!(opts.image, "ghcr.io/orcher-io/orcher:0.5.4");
        assert_eq!((opts.grpc_port, opts.http_port), (50061, 8081));
        assert_eq!(opts.bind_host, "0.0.0.0");
        assert_eq!(opts.log_level, "debug");
        assert_eq!(
            opts.database_url, None,
            "the local database is not external"
        );
        assert_eq!(opts.config_file, Some(PathBuf::from("/home/me/auth.toml")));
    }

    #[test]
    fn port_conflicts_are_recognized() {
        let err = CliError::local_engine(
            "docker run failed: Bind for 127.0.0.1:50051 failed: port is already allocated",
        );
        assert!(port_in_use(&err));
        assert!(!port_in_use(&CliError::local_engine(
            "docker run failed: no such image"
        )));
    }
}
