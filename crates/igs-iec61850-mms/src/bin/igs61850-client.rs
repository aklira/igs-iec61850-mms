// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! `igs61850-client`: a command line client of the IEC 61850 model over MMS (task A7). It lists the logical devices and
//! their nodes, discovers the functional groups and the definitions of the data objects, and reads and writes attributes.
//! Each item of a read or a write is printed with its own result. Exit status: 0 when every request succeeded, 1 when a
//! request failed as a whole or an item failed, 2 for a bad command line.

use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use igs_iec61850_mms::client::{ClientError, IecClient};
use igs_iec61850_mms::model::client_config;
use igs_iec61850_mms_core::discovery::{Definition, Leaf, Size};
use igs_iec61850_mms_core::names::{MmsIdentifier, MmsName, Name, Scope};
use igs_iso9506_mms::Reconnect;
use igs_iso9506_mms_codec::generated::types::Data;
use igs_iso9506_mms_link::config::LinkConfig;

#[derive(Parser)]
#[command(
    name = "igs61850-client",
    version,
    about = "Discovers and reads an IEC 61850 server over MMS"
)]
struct Cli {
    /// The server, as host:port.
    #[arg(long, default_value = "127.0.0.1:11102")]
    server: String,
    /// Accept a server that negotiates no parameter CBB (gap G8), as the MMS client does.
    #[arg(long)]
    lenient_parameter_cbbs: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// The logical devices of the server.
    Domains,
    /// The logical nodes of a logical device.
    Nodes { device: String },
    /// The named variables of a logical device (the nodes and the flattened names of their attributes).
    Names { device: String },
    /// The functional constraints of a logical node, with their data objects.
    Groups { device: String, node: String },
    /// The definition of a data object under a functional constraint.
    Definition {
        device: String,
        node: String,
        fc: String,
        object: String,
    },
    /// Reads items of a logical device, as `NODE$FC$OBJECT[$ATTRIBUTE...]`.
    Read {
        device: String,
        #[arg(required = true)]
        items: Vec<String>,
    },
    /// Writes a boolean (`true`, `false`) or an integer to an item.
    Write {
        device: String,
        item: String,
        value: String,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(Failure::Usage(message)) => {
            eprintln!("igs61850-client: {message}");
            ExitCode::from(2)
        }
        Err(Failure::Client(error)) => {
            eprintln!("igs61850-client: {error}");
            ExitCode::from(1)
        }
    }
}

enum Failure {
    Usage(String),
    Client(ClientError),
}

impl From<ClientError> for Failure {
    fn from(error: ClientError) -> Self {
        Self::Client(error)
    }
}

/// Runs the command. `Ok(false)` when an item failed, although the request as a whole succeeded.
async fn run(cli: Cli) -> Result<bool, Failure> {
    let mut config = client_config(LinkConfig::default());
    config.lenient_parameter_cbbs = cli.lenient_parameter_cbbs;
    config.reconnect = Reconnect::Never;
    let (client, _events) = IecClient::connect(&cli.server, config).await?;
    let outcome = execute(&client, cli.command).await;
    // A server that is slow to conclude does not hold the command: the association is dropped after two seconds.
    let _ = tokio::time::timeout(Duration::from_secs(2), client.close()).await;
    outcome
}

async fn execute(client: &IecClient, command: Command) -> Result<bool, Failure> {
    match command {
        Command::Domains => {
            for domain in client.domains().await? {
                println!("{domain}");
            }
        }
        Command::Nodes { device } => {
            for node in client.logical_nodes(&device).await? {
                println!("{node}");
            }
        }
        Command::Names { device } => {
            for name in client.named_variables(&device).await? {
                println!("{name}");
            }
        }
        Command::Groups { device, node } => {
            for group in client.functional_groups(&device, &node).await? {
                println!(
                    "{}: {}",
                    group.functional_constraint,
                    group.objects.join(" ")
                );
            }
        }
        Command::Definition {
            device,
            node,
            fc,
            object,
        } => {
            let definition = client.definition(&device, &node, &fc, &object).await?;
            print_definition(&object, definition.definition(), 0);
        }
        Command::Read { device, items } => {
            let names = items
                .iter()
                .map(|item| item_of(&device, item))
                .collect::<Result<Vec<_>, _>>()?;
            let results = client.read(&names).await?;
            let mut all_ok = true;
            for (item, result) in items.iter().zip(results) {
                match result {
                    Ok(value) => println!("{item} = {}", show(&value)),
                    Err(error) => {
                        all_ok = false;
                        println!("{item} : {error:?}");
                    }
                }
            }
            return Ok(all_ok);
        }
        Command::Write {
            device,
            item,
            value,
        } => {
            let name = item_of(&device, &item)?;
            let data = parse_value(&value)
                .ok_or_else(|| Failure::Usage(format!("{value} is not a value")))?;
            let results = client.write(&[(name, data)]).await?;
            if let Some(Err(error)) = results.first() {
                println!("{item} : {error:?}");
                return Ok(false);
            }
            println!("{item} written");
        }
    }
    Ok(true)
}

/// The MMS name of an item of a logical device.
fn item_of(device: &str, item: &str) -> Result<MmsName, Failure> {
    let device = Name::new(device).map_err(|error| Failure::Usage(error.to_string()))?;
    let identifier = MmsIdentifier::new(item).map_err(|error| Failure::Usage(error.to_string()))?;
    Ok(MmsName::new(Scope::Domain(device), identifier))
}

fn parse_value(text: &str) -> Option<Data> {
    match text {
        "true" => Some(Data::Boolean(true)),
        "false" => Some(Data::Boolean(false)),
        _ => text.parse::<i128>().ok().map(Data::Integer),
    }
}

fn show(value: &Data) -> String {
    match value {
        Data::Boolean(value) => value.to_string(),
        Data::Integer(value) => value.to_string(),
        Data::Unsigned(value) => value.to_string(),
        Data::VisibleString(text) => format!("{text:?}"),
        Data::FloatingPoint(float) => match float.to_f32() {
            Ok(value) => value.to_string(),
            Err(_) => format!("{:?}", float.0),
        },
        other => format!("{other:?}"),
    }
}

fn print_definition(name: &str, definition: &Definition, depth: usize) {
    let indent = "  ".repeat(depth);
    match definition {
        Definition::Leaf(leaf) => println!("{indent}{name}: {}", describe_leaf(leaf)),
        Definition::Structure(members) => {
            println!("{indent}{name}:");
            for member in members {
                print_definition(&member.name, &member.definition, depth.saturating_add(1));
            }
        }
        Definition::Array { elements, element } => {
            println!("{indent}{name}: array of {elements}");
            print_definition("[]", element, depth.saturating_add(1));
        }
    }
}

fn describe_size(size: Size, unit: &str) -> String {
    if size.variable {
        format!("at most {} {unit}", size.length)
    } else {
        format!("{} {unit}", size.length)
    }
}

fn describe_leaf(leaf: &Leaf) -> String {
    match leaf {
        Leaf::Boolean => "boolean".to_string(),
        Leaf::Integer { bits } => format!("integer of {bits} bits"),
        Leaf::Unsigned { bits } => format!("unsigned of {bits} bits"),
        Leaf::FloatingPoint {
            format_width,
            exponent_width,
        } => format!("floating point {format_width} bits, exponent {exponent_width}"),
        Leaf::BitString { size } => format!("bit string of {}", describe_size(*size, "bits")),
        Leaf::OctetString { size } => format!("octet string of {}", describe_size(*size, "octets")),
        Leaf::VisibleString { size } => {
            format!("visible string of {}", describe_size(*size, "characters"))
        }
        Leaf::MmsString { size } => format!("string of {}", describe_size(*size, "characters")),
        Leaf::UtcTime => "UTC time".to_string(),
        other => format!("{other:?}"),
    }
}
