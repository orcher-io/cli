//! `orcher namespace` — create, get, list, update, deprecate, and delete namespaces.

use crate::error::{CliError, Result};
use crate::render;
use crate::time::Timestamptz;
use crate::utils::GlobalConfig;
use clap::{Args, Subcommand};
use console::style;
use tracing::info;

/// Namespace management command
#[derive(Args)]
pub struct NamespaceCommand {
    #[command(subcommand)]
    pub action: NamespaceCommands,
}

#[derive(Subcommand)]
pub enum NamespaceCommands {
    /// Create a new namespace
    Create {
        /// Namespace name (alphanumeric, hyphens, underscores, 1-100 chars)
        name: String,

        /// Human-readable description
        #[arg(short = 'd', long = "description", default_value = "")]
        description: String,

        /// Retention period for completed workflow data (in days)
        #[arg(short = 'r', long = "retention-days", default_value = "7")]
        retention_days: i32,

        /// Owner email address
        #[arg(short = 'e', long = "owner-email", default_value = "")]
        owner_email: String,

        /// Key-value data pairs (key=value)
        #[arg(long = "data")]
        data: Vec<String>,
    },

    /// Get detailed information about a namespace
    #[command(aliases = ["describe", "info"])]
    Get {
        /// Namespace name
        name: String,
    },

    /// List all namespaces
    #[command(aliases = ["ls"])]
    List {
        /// Maximum number of results
        #[arg(short = 'l', long = "limit", default_value = "100")]
        limit: i32,

        /// Offset for pagination
        #[arg(long = "offset", default_value = "0")]
        offset: i32,
    },

    /// Update namespace metadata
    Update {
        /// Namespace name to update
        name: String,

        /// Updated description
        #[arg(short = 'd', long = "description")]
        description: Option<String>,

        /// Updated owner email
        #[arg(short = 'e', long = "owner-email")]
        owner_email: Option<String>,

        /// Updated retention period (in days)
        #[arg(short = 'r', long = "retention-days")]
        retention_days: Option<i32>,

        /// Updated key-value data pairs (key=value, replaces all existing data)
        #[arg(long = "data")]
        data: Vec<String>,
    },

    /// Deprecate a namespace (no new workflows allowed)
    Deprecate {
        /// Namespace name to deprecate
        name: String,

        /// Skip confirmation prompt
        #[arg(short = 'f', long = "force")]
        force: bool,
    },

    /// Delete a namespace (soft delete)
    Delete {
        /// Namespace name to delete
        name: String,

        /// Skip confirmation prompt
        #[arg(short = 'f', long = "force")]
        force: bool,
    },
}

/// Execute namespace management command
pub async fn execute(cmd: NamespaceCommand, global_config: &GlobalConfig) -> Result<()> {
    match cmd.action {
        NamespaceCommands::Create {
            name,
            description,
            retention_days,
            owner_email,
            data,
        } => {
            create_namespace(
                name,
                description,
                retention_days,
                owner_email,
                data,
                global_config,
            )
            .await
        }
        NamespaceCommands::Get { name } => get_namespace(name, global_config).await,
        NamespaceCommands::List { limit, offset } => {
            list_namespaces(limit, offset, global_config).await
        }
        NamespaceCommands::Update {
            name,
            description,
            owner_email,
            retention_days,
            data,
        } => {
            update_namespace(
                name,
                description,
                owner_email,
                retention_days,
                data,
                global_config,
            )
            .await
        }
        NamespaceCommands::Deprecate { name, force } => {
            deprecate_namespace(name, force, global_config).await
        }
        NamespaceCommands::Delete { name, force } => {
            delete_namespace(name, force, global_config).await
        }
    }
}

/// Create a new namespace
async fn create_namespace(
    name: String,
    description: String,
    retention_days: i32,
    owner_email: String,
    data: Vec<String>,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!("Creating namespace: {}", name);

    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    let data_map = crate::utils::parse_key_value_pairs(data)?;

    let ns = client
        .create_namespace(&name, &description, retention_days, &owner_email, data_map)
        .await?;

    if global_config.is_structured_output() {
        print_namespace_structured(&ns, &global_config.output_format);
    } else {
        println!(
            "{} Namespace {} created successfully",
            style("Created").green().bold(),
            style(&ns.name).cyan().bold()
        );
        println!();
        print_namespace_detail(&ns);
    }

    Ok(())
}

/// Get namespace details
async fn get_namespace(name: String, global_config: &GlobalConfig) -> Result<()> {
    info!("Getting namespace: {}", name);

    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    let ns = client.get_namespace(&name).await?;

    if global_config.is_structured_output() {
        print_namespace_structured(&ns, &global_config.output_format);
    } else {
        print_namespace_detail(&ns);
    }

    Ok(())
}

/// List namespaces
async fn list_namespaces(limit: i32, offset: i32, global_config: &GlobalConfig) -> Result<()> {
    info!("Listing namespaces");

    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    let (namespaces, total_count) = client.list_namespaces(limit, offset).await?;

    if namespaces.is_empty() {
        println!("{}", style("No namespaces found").yellow());
        return Ok(());
    }

    if global_config.is_structured_output() {
        let ns_list: Vec<serde_json::Value> = namespaces.iter().map(namespace_to_json).collect();
        match global_config.output_format.as_str() {
            "json" => println!(
                "{}",
                serde_json::to_string_pretty(&ns_list).unwrap_or_default()
            ),
            "yaml" => println!("{}", serde_yaml::to_string(&ns_list).unwrap_or_default()),
            _ => {}
        }
        return Ok(());
    }

    if global_config.quiet {
        for ns in &namespaces {
            println!("{}", ns.name);
        }
        return Ok(());
    }

    let rows: Vec<Vec<String>> = namespaces
        .iter()
        .map(|ns| {
            let description = if ns.description.chars().count() > 40 {
                format!("{}…", ns.description.chars().take(39).collect::<String>())
            } else {
                render::or_dash(&ns.description)
            };
            vec![
                render::or_dash(&ns.name),
                render::status_cell_str(&ns.status),
                format!("{}d", ns.retention_period_days),
                description,
                render::relative_time(ns.created_at.as_ref().map(|t| t.seconds)),
            ]
        })
        .collect();

    println!();
    println!(
        "{}",
        render::table(
            &["NAME", "STATUS", "RETENTION", "DESCRIPTION", "CREATED"],
            rows
        )
    );
    println!();
    println!(
        "  {}",
        style(format!(
            "{} of {} namespace{}",
            namespaces.len(),
            total_count,
            if total_count == 1 { "" } else { "s" }
        ))
        .dim()
    );

    Ok(())
}

/// Update namespace metadata
async fn update_namespace(
    name: String,
    description: Option<String>,
    owner_email: Option<String>,
    retention_days: Option<i32>,
    data: Vec<String>,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!("Updating namespace: {}", name);

    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    let data_map = crate::utils::parse_key_value_pairs(data)?;

    // Empty string means "no change" in the proto contract
    let ns = client
        .update_namespace(
            &name,
            description.as_deref().unwrap_or(""),
            owner_email.as_deref().unwrap_or(""),
            retention_days.unwrap_or(0),
            data_map,
        )
        .await?;

    if global_config.is_structured_output() {
        print_namespace_structured(&ns, &global_config.output_format);
    } else {
        println!(
            "{} Namespace {} updated successfully",
            style("Updated").green().bold(),
            style(&ns.name).cyan().bold()
        );
        println!();
        print_namespace_detail(&ns);
    }

    Ok(())
}

/// Deprecate a namespace
async fn deprecate_namespace(
    name: String,
    force: bool,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!("Deprecating namespace: {}", name);

    if !force {
        let confirmed = crate::commands::common::confirm_action(
            &format!(
                "Deprecate namespace '{}'? No new workflows will be allowed.",
                name
            ),
            false,
        )?;
        if !confirmed {
            println!("{}", style("Cancelled").yellow());
            return Ok(());
        }
    }

    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    let ns = client.deprecate_namespace(&name).await?;

    if global_config.is_structured_output() {
        print_namespace_structured(&ns, &global_config.output_format);
    } else {
        println!(
            "{} Namespace {} has been deprecated",
            style("Deprecated").yellow().bold(),
            style(&ns.name).cyan().bold()
        );
        println!("No new workflows can be started in this namespace.");
    }

    Ok(())
}

/// Delete a namespace
async fn delete_namespace(name: String, force: bool, global_config: &GlobalConfig) -> Result<()> {
    info!("Deleting namespace: {}", name);

    if name == "default" && !force {
        return Err(CliError::invalid_input(
            "Cannot delete the 'default' namespace. Use --force to override.",
        ));
    }

    if !force {
        let confirmed = crate::commands::common::confirm_action(
            &format!(
                "Delete namespace '{}'? This is a soft-delete and can be reversed by an admin.",
                name
            ),
            false,
        )?;
        if !confirmed {
            println!("{}", style("Cancelled").yellow());
            return Ok(());
        }
    }

    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    client.delete_namespace(&name).await?;

    if !global_config.quiet {
        println!(
            "{} Namespace {} has been deleted",
            style("Deleted").red().bold(),
            style(&name).cyan().bold()
        );
    }

    Ok(())
}

// =========================================================================
// Output Helpers
// =========================================================================

fn print_namespace_detail(ns: &orcher_proto::NamespaceInfo) {
    println!("{}", style("Namespace Details").bold());
    println!("{}", style("-".repeat(50)).dim());
    println!("  Name:            {}", style(&ns.name).cyan().bold());

    let status_display = match ns.status.as_str() {
        "active" => "ACTIVE".to_string(),
        "deprecated" => "DEPRECATED".to_string(),
        "deleted" => "DELETED".to_string(),
        other => other.to_uppercase(),
    };
    let status_styled = match ns.status.as_str() {
        "active" => style(status_display).green().bold(),
        "deprecated" => style(status_display).yellow().bold(),
        "deleted" => style(status_display).red().bold(),
        _ => style(status_display).white().bold(),
    };
    println!("  Status:          {}", status_styled);

    if !ns.description.is_empty() {
        println!("  Description:     {}", ns.description);
    }

    if !ns.owner_email.is_empty() {
        println!("  Owner:           {}", ns.owner_email);
    }

    println!("  Retention:       {} days", ns.retention_period_days);

    if let Some(created) = &ns.created_at {
        let dt = Timestamptz::from_second(created.seconds)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| "-".to_string());
        println!("  Created:         {}", dt);
    }

    if let Some(updated) = &ns.updated_at {
        let dt = Timestamptz::from_second(updated.seconds)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| "-".to_string());
        println!("  Updated:         {}", dt);
    }

    if !ns.data.is_empty() {
        println!("  Data:");
        for (key, value) in &ns.data {
            println!("    {}: {}", style(key).dim(), value);
        }
    }
}

fn print_namespace_structured(ns: &orcher_proto::NamespaceInfo, format: &str) {
    let json_val = namespace_to_json(ns);
    match format {
        "json" => println!(
            "{}",
            serde_json::to_string_pretty(&json_val).unwrap_or_default()
        ),
        "yaml" => println!("{}", serde_yaml::to_string(&json_val).unwrap_or_default()),
        _ => {}
    }
}

fn namespace_to_json(ns: &orcher_proto::NamespaceInfo) -> serde_json::Value {
    let created = ns
        .created_at
        .as_ref()
        .map(|t| {
            Timestamptz::from_second(t.seconds)
                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_default()
        })
        .unwrap_or_default();

    let updated = ns
        .updated_at
        .as_ref()
        .map(|t| {
            Timestamptz::from_second(t.seconds)
                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_default()
        })
        .unwrap_or_default();

    serde_json::json!({
        "name": ns.name,
        "status": ns.status,
        "description": ns.description,
        "owner_email": ns.owner_email,
        "retention_period_days": ns.retention_period_days,
        "data": ns.data,
        "created_at": created,
        "updated_at": updated,
    })
}
