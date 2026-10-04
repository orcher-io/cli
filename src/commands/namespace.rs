//! `orcher namespace`: the namespaces workflows run in.

use crate::client::Client;
use crate::commands::confirm;
use crate::error::{Error, Result};
use crate::render;
use crate::settings::{GlobalArgs, Output};
use clap::{Args, Subcommand};
use console::style;
use orcher_proto::{CreateNamespaceRequest, NamespaceInfo, UpdateNamespaceRequest};
use std::collections::HashMap;

#[derive(Args)]
pub struct NamespaceCommand {
    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    /// List namespaces
    #[command(alias = "ls")]
    List {
        /// How many to show
        #[arg(short, long, default_value_t = 100)]
        limit: i32,

        /// How many to skip
        #[arg(long, default_value_t = 0)]
        offset: i32,
    },

    /// Show a namespace
    #[command(alias = "get")]
    Describe { name: String },

    /// Create a namespace
    Create {
        /// Letters, digits, hyphens and underscores; up to 100 characters
        name: String,

        /// What the namespace is for
        #[arg(short, long, default_value = "")]
        description: String,

        /// How many days to keep finished workflows
        #[arg(short, long, default_value_t = 7)]
        retention_days: i32,

        /// Who to contact about it
        #[arg(long, default_value = "")]
        owner_email: String,

        /// Extra data, as key=value; repeatable
        #[arg(long = "data", value_name = "KEY=VALUE")]
        data: Vec<String>,
    },

    /// Change a namespace's details
    Update {
        name: String,

        #[arg(short, long)]
        description: Option<String>,

        #[arg(short, long)]
        retention_days: Option<i32>,

        #[arg(long)]
        owner_email: Option<String>,

        /// Extra data, as key=value; repeatable. Replaces all existing data.
        #[arg(long = "data", value_name = "KEY=VALUE")]
        data: Vec<String>,
    },

    /// Stop new workflows from starting in a namespace
    Deprecate {
        name: String,

        /// Do not ask for confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Delete a namespace
    Delete {
        name: String,

        /// Do not ask for confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

pub async fn run(cmd: NamespaceCommand, args: &GlobalArgs) -> Result<()> {
    match cmd.action {
        Action::List { limit, offset } => list(limit, offset, args).await,
        Action::Describe { name } => {
            let ns = Client::connect(args).await?.get_namespace(&name).await?;
            show(&ns, args)
        }
        Action::Create {
            name,
            description,
            retention_days,
            owner_email,
            data,
        } => {
            let request = CreateNamespaceRequest {
                name,
                description,
                retention_period_days: retention_days,
                owner_email,
                data: key_values(data)?,
            };
            let ns = Client::connect(args)
                .await?
                .create_namespace(request)
                .await?;
            show(&ns, args)
        }
        Action::Update {
            name,
            description,
            retention_days,
            owner_email,
            data,
        } => {
            // The engine leaves a field unchanged when it is empty or zero.
            let request = UpdateNamespaceRequest {
                name,
                description: description.unwrap_or_default(),
                owner_email: owner_email.unwrap_or_default(),
                retention_period_days: retention_days.unwrap_or(0),
                data: key_values(data)?,
            };
            let ns = Client::connect(args)
                .await?
                .update_namespace(request)
                .await?;
            show(&ns, args)
        }
        Action::Deprecate { name, yes } => {
            if !confirm(
                &format!("Deprecate namespace '{name}'? No new workflows can start in it."),
                yes,
            )? {
                return Err(Error::invalid_input("not confirmed; nothing was changed"));
            }
            let ns = Client::connect(args)
                .await?
                .deprecate_namespace(&name)
                .await?;
            show(&ns, args)
        }
        Action::Delete { name, yes } => {
            if !confirm(&format!("Delete namespace '{name}'?"), yes)? {
                return Err(Error::invalid_input("not confirmed; nothing was deleted"));
            }
            Client::connect(args).await?.delete_namespace(&name).await?;
            if !args.quiet {
                println!("Namespace '{name}' deleted.");
            }
            Ok(())
        }
    }
}

async fn list(limit: i32, offset: i32, args: &GlobalArgs) -> Result<()> {
    let (namespaces, total) = Client::connect(args)
        .await?
        .list_namespaces(limit, offset)
        .await?;

    let items = namespaces.iter().map(to_json).collect();
    if render::structured(args, &serde_json::Value::Array(items))? {
        return Ok(());
    }
    if args.output == Output::Name {
        for ns in &namespaces {
            println!("{}", ns.name);
        }
        return Ok(());
    }
    if namespaces.is_empty() {
        if !args.quiet {
            println!("No namespaces found.");
        }
        return Ok(());
    }

    let rows = namespaces
        .iter()
        .map(|ns| {
            vec![
                ns.name.clone(),
                render::status_cell_str(&ns.status),
                format!("{}d", ns.retention_period_days),
                render::or_dash(&ellipsize(&ns.description, 40)),
                render::relative_time(ns.created_at.as_ref()),
            ]
        })
        .collect();
    println!(
        "{}",
        render::table(
            &["NAME", "STATUS", "RETENTION", "DESCRIPTION", "CREATED"],
            rows
        )
    );
    if !args.quiet {
        let summary = format!("{} of {total}", namespaces.len());
        println!("{}", style(summary).dim());
    }
    Ok(())
}

fn show(ns: &NamespaceInfo, args: &GlobalArgs) -> Result<()> {
    if render::structured(args, &to_json(ns))? {
        return Ok(());
    }
    if args.output == Output::Name {
        println!("{}", ns.name);
        return Ok(());
    }
    let field = |name: &str, value: String| println!("  {:<15}{}", style(name).dim(), value);
    println!("{}", style(&ns.name).bold());
    field("Status", render::status_cell_str(&ns.status));
    field("Retention", format!("{} days", ns.retention_period_days));
    if !ns.description.is_empty() {
        field("Description", ns.description.clone());
    }
    if !ns.owner_email.is_empty() {
        field("Owner", ns.owner_email.clone());
    }
    field("Created", render::absolute_time(ns.created_at.as_ref()));
    field("Updated", render::absolute_time(ns.updated_at.as_ref()));
    let mut data: Vec<_> = ns.data.iter().collect();
    data.sort();
    for (key, value) in data {
        field(key, value.clone());
    }
    Ok(())
}

fn to_json(ns: &NamespaceInfo) -> serde_json::Value {
    serde_json::json!({
        "name": ns.name,
        "status": ns.status,
        "description": ns.description,
        "ownerEmail": ns.owner_email,
        "retentionDays": ns.retention_period_days,
        "data": ns.data,
        "createdAt": render::rfc3339(ns.created_at.as_ref()),
        "updatedAt": render::rfc3339(ns.updated_at.as_ref()),
    })
}

fn ellipsize(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let head: String = text.chars().take(max - 1).collect();
        format!("{head}…")
    }
}

/// Parses repeated `key=value` arguments.
fn key_values(pairs: Vec<String>) -> Result<HashMap<String, String>> {
    pairs
        .into_iter()
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) if !key.is_empty() => Ok((key.to_string(), value.to_string())),
            _ => Err(Error::invalid_input(format!(
                "'{pair}' is not in the form key=value"
            ))),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_values_parse() {
        let parsed = key_values(vec!["team=payments".into(), "url=a=b".into()]).unwrap();
        assert_eq!(parsed["team"], "payments");
        assert_eq!(parsed["url"], "a=b", "only the first = separates");
        assert!(key_values(vec!["novalue".into()]).is_err());
        assert!(key_values(vec!["=x".into()]).is_err());
    }

    #[test]
    fn long_descriptions_are_shortened() {
        assert_eq!(ellipsize("short", 40), "short");
        assert_eq!(ellipsize(&"x".repeat(50), 10).chars().count(), 10);
    }
}
