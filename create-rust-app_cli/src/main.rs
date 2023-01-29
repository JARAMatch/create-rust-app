extern crate inflector;

mod content;
mod plugins;
mod qsync;
mod utils;

use anyhow::Result;
use clap::{
    builder::{EnumValueParser, PossibleValue, ValueHint},
    Parser, Subcommand, ValueEnum,
};
use std::path::PathBuf;

use crate::project::CreationOptions;
use content::project;
use dialoguer::{console::Term, theme::ColorfulTheme, Input, MultiSelect, Select};
use utils::{fs, logger};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum BackendFramework {
    ActixWeb,
    Poem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum BackendDatabase {
    Postgres,
    Sqlite,
}

/// Struct to describe the CLI
#[derive(Parser)]
#[command(name = "create-rust-app", author, version, about, long_about)]
struct Cli {
    /// subcommands
    #[command(subcommand)]
    command: Commands,
}

/// enum for the various available subcommands
#[derive(Subcommand)]
enum Commands {
    /// Create a new rust app
    Create {
        #[arg(
            short = 'i',
            long = "interactive",
            name = "interactive mode",
            help = "Configure project through interactive TUI."
        )]
        interactive: bool,

        #[arg(
            help = "Name of new project",
            value_hint = ValueHint::DirPath,
        )]
        name: String,

        #[arg(
            short='d',
            long="database",
            name="database",
            help="Database to use",
            require_equals=true,
            value_name="DATABASE",
            value_parser=EnumValueParser::<BackendDatabase>::new(),
            ignore_case=true,
            required_unless_present="interactive mode",
        )]
        database: Option<BackendDatabase>,

        #[arg(
            short='b',
            long="backend",
            name="backend framework",
            help="Rust backend framework to use",
            require_equals=true,
            value_name="FRAMEWORK",
            value_parser=EnumValueParser::<BackendFramework>::new(),
            ignore_case=true,
            required_unless_present="interactive mode",
        )]
        backendframework: Option<BackendFramework>,

        //TODO: create an enum for the plugins if we can maintain the help information
        //TODO: add utoipa to this list when it's merged
        #[arg(
            long="plugins",
            name="plugins",
            help="Plugins for your new project\nComma separated list ",
            num_args=1..,
            value_delimiter=',',
            require_equals=true,
            value_name="PLUGINS",
            value_parser=[
                PossibleValue::new("auth").help("Authentication Plugin: local email-based authentication"),
                PossibleValue::new("container").help("Container Plugin: dockerize your app"),
                PossibleValue::new("dev").help("Development Plugin: adds dev warnings and an admin portal"),
                PossibleValue::new("storage").help("Storage Plugin: adds S3 file storage capabilities"),
                PossibleValue::new("graphql").help("GraphQL Plugin: bootstraps a GraphQL setup including a playground"),
            ],
            ignore_case=true,
            required_unless_present="interactive mode",
        )]
        plugins: Option<Vec<String>>,
    },
    // named Configure instead of Update because people would naturally assume that Update updates the version of the CLI
    /// Configure an existing rust project
    Configure {
        //TODO: Consider splitting these into 2 separate subcommands
        #[arg(
            long = "qsync",
            name = "query-sync",
            help = "Generate react-query hooks for frontend. (beta)\nOnly supports Actix backend",
            conflicts_with = "add new service"
        )]
        query_sync: bool,

        #[arg(
            long="input",
            name="input files",
            value_name = "INPUT",
            num_args=1..,
            value_delimiter=',',
            require_equals=true,
            value_hint = ValueHint::FilePath,
            value_hint = ValueHint::DirPath,
            conflicts_with = "add new service",
            hide = true,
            help = "rust file(s) to read type information from",
        )]
        qsync_input_files: Option<Vec<PathBuf>>,

        #[arg(
            long="output",
            name="output file",
            value_name = "OUTPUT",
            num_args=1,
            value_hint = ValueHint::FilePath,
            value_hint = ValueHint::DirPath,
            conflicts_with = "add new service",
            hide = true,
            help = "file to write generated types to",
        )]
        qsync_output_file: Option<PathBuf>,

        #[arg(
            short = 'd',
            name = "Debug",
            help = "Dry-run, prints to stdout",
            hide = true
        )]
        qsync_debug: bool,

        #[arg(
            long = "new-service",
            name = "add new service",
            help = "Add a model & service for backend. (beta)",
            conflicts_with = "query-sync"
        )]
        add_new_service: bool,
    },
}

/// CREATE RUST APP
///
/// A MODERN WAY TO BOOTSTRAP A RUST+REACT APP IN A SINGLE COMMAND
fn main() -> Result<()> {
    let cli = Cli::parse();

    project::check_cli_version()?;

    // You can check for the existence of subcommands, and if found use their
    // matches just as you would the top level cmd
    // note, takes ownership of cli
    match cli.command {
        Commands::Create {
            interactive,
            name,
            database,
            backendframework,
            plugins,
        } => {
            create_project(interactive, name, database, backendframework, plugins)?;
        }
        Commands::Configure {
            query_sync,
            qsync_input_files,
            qsync_output_file,
            qsync_debug,
            add_new_service,
        } => {
            configure_project(
                query_sync,
                qsync_input_files,
                qsync_output_file,
                qsync_debug,
                add_new_service,
            )?;
        }
    }

    Ok(())
}

fn create_project(
    interactive: bool,
    project_name: String,
    database: Option<BackendDatabase>,
    framework: Option<BackendFramework>,
    plugins: Option<Vec<String>>,
) -> anyhow::Result<()> {
    // if we try making a project in an existing directory, throw an error
    if PathBuf::from(&project_name).exists() {
        logger::error(&format!(
            "Cannot create a project: {:#?} already exists.",
            PathBuf::from(&project_name)
        ));
        return Ok(());
    }

    // get the backend database
    let backend_database = match database {
        Some(database) => database,
        None => {
            if interactive {
                logger::message("Select a database to use:");
                logger::message("Use UP/DOWN arrows to navigate and SPACE or ENTER to confirm.");
                let items = vec!["postgres", "sqlite"];
                let selection = Select::with_theme(&ColorfulTheme::default())
                    .items(&items)
                    .default(0)
                    .interact_on_opt(&Term::stderr())?;

                match selection {
                    Some(0) => BackendDatabase::Postgres,
                    Some(1) => BackendDatabase::Sqlite,
                    _ => panic!("Fatal: Unknown backend database specified."),
                }
            } else {
                panic!("Fatal: No backend database specified")
            }
        }
    };

    // get the backend framework
    let backend_framework: BackendFramework = match framework {
        Some(framework) => framework,
        None => {
            if interactive {
                logger::message("Select a rust backend framework to use:");
                logger::message("Use UP/DOWN arrows to navigate and SPACE or ENTER to confirm.");
                let items = vec!["actix-web", "poem"];
                let selection = Select::with_theme(&ColorfulTheme::default())
                    .items(&items)
                    .default(0)
                    .interact_on_opt(&Term::stderr())?;

                match selection {
                    Some(0) => BackendFramework::ActixWeb,
                    Some(1) => BackendFramework::Poem,
                    _ => panic!("Fatal: Unknown backend framework specified."),
                }
            } else {
                panic!("Fatal: No backend database specified")
            }
        }
    };

    // get enabled features (plugins)
    let mut cra_enabled_features: Vec<String> = match plugins {
        Some(plugins) => plugins
            .iter()
            .map(|plugin| match plugin.as_str() {
                "auth" => "plugin_auth".to_string(),
                "container" => "plugin_container".to_string(),
                "dev" => "plugin_dev".to_string(),
                "storage" => "plugin_storage".to_string(),
                "graphql" => "plugin_graphql".to_string(),
                _ => panic!("Fatal: Unknown plugin specified"),
            })
            .collect(),
        None => {
            if interactive {
                logger::message("Please select plugins for your new project:");
                logger::message(
                    "Use UP/DOWN arrows to navigate, SPACE to enable/disable a plugin, and ENTER to confirm.",
                );

                let items = vec![
        "Authentication Plugin: local email-based authentication",
        "Container Plugin: dockerize your app",
        "Development Plugin: adds dev warnings and an admin portal",
        "Storage Plugin: adds S3 file storage capabilities",
        "GraphQL Plugin: bootstraps a GraphQL setup including a playground",
        "Utoipa Plugin: Autogenerated OpenAPI documentation served in a SwaggerUI playground",
    ];
                let chosen: Vec<usize> = MultiSelect::with_theme(&ColorfulTheme::default())
                    .items(&items)
                    .defaults(&[true, true, true, true, true, false])
                    .interact()?;

                let add_plugin_auth = chosen.iter().any(|x| *x == 0);
                let add_plugin_container = chosen.iter().any(|x| *x == 1);
                let add_plugin_dev = chosen.iter().any(|x| *x == 2);
                let add_plugin_storage = chosen.iter().any(|x| *x == 3);
                let add_plugin_graphql = chosen.iter().any(|x| *x == 4);
                let add_plugin_utoipa = chosen.iter().any(|x| *x == 5);

                let mut features: Vec<String> = vec![];
                if add_plugin_auth {
                    features.push("plugin_auth".to_string());
                }
                if add_plugin_container {
                    features.push("plugin_container".to_string());
                }
                if add_plugin_dev {
                    features.push("plugin_dev".to_string());
                }
                if add_plugin_storage {
                    features.push("plugin_storage".to_string());
                }
                if add_plugin_graphql {
                    features.push("plugin_graphql".to_string());
                }
                if add_plugin_utoipa {
                    features.push("plugin_utoipa".to_string());
                }
                features
            } else {
                panic!("Fatal: No plugins specified")
            }
        }
    };
    // add database and framework to enabled features
    cra_enabled_features.push(match backend_database {
        BackendDatabase::Postgres => "database_postgres".to_string(),
        BackendDatabase::Sqlite => "database_sqlite".to_string(),
    });
    cra_enabled_features.push(match backend_framework {
        BackendFramework::ActixWeb => "backend_actix-web".to_string(),
        BackendFramework::Poem => "backend_poem".to_string(),
    });

    project::create(
        project_name.as_ref(),
        CreationOptions {
            cra_enabled_features: cra_enabled_features.clone(),
            backend_framework,
            backend_database,
        },
    )?;

    let mut project_dir = PathBuf::from(".");
    project_dir.push(project_name);
    // !
    std::env::set_current_dir(project_dir.clone())
        .unwrap_or_else(|_| panic!("Unable to change into {:#?}", project_dir.clone()));

    //
    // Note: we're in the project dir from here on out
    //

    let install_config = plugins::InstallConfig {
        project_dir: PathBuf::from("."),
        backend_framework,
        backend_database,
        plugin_auth: cra_enabled_features
            .iter()
            .any(|feature| feature == "plugin_auth"),
        plugin_container: cra_enabled_features
            .iter()
            .any(|feature| feature == "plugin_container"),
        plugin_dev: cra_enabled_features
            .iter()
            .any(|feature| feature == "plugin_dev"),
        plugin_storage: cra_enabled_features
            .iter()
            .any(|feature| feature == "plugin_storage"),
        plugin_graphql: cra_enabled_features
            .iter()
            .any(|feature| feature == "plugin_graphql"),
        plugin_utoipa: cra_enabled_features
            .iter()
            .any(|feature| feature == "plugin_utoipa"),
    };

    if cra_enabled_features
        .iter()
        .any(|feature| feature == "plugin_auth")
    {
        plugins::install(plugins::auth::Auth {}, install_config.clone())?;
    }
    if cra_enabled_features
        .iter()
        .any(|feature| feature == "plugin_container")
    {
        plugins::install(plugins::container::Container {}, install_config.clone())?;
    }
    if cra_enabled_features
        .iter()
        .any(|feature| feature == "plugin_dev")
    {
        plugins::install(plugins::dev::Dev {}, install_config.clone())?;
    }
    if cra_enabled_features
        .iter()
        .any(|feature| feature == "plugin_storage")
    {
        plugins::install(plugins::storage::Storage {}, install_config.clone())?;
    }
    if cra_enabled_features
        .iter()
        .any(|feature| feature == "plugin_graphql")
    {
        plugins::install(plugins::graphql::GraphQL {}, install_config.clone())?;
    }
    if cra_enabled_features
        .iter()
        .any(|feature| feature == "plugin_utoipa")
    {
        plugins::install(plugins::utoipa::Utoipa {}, install_config.clone())?;
    }

    // cd into project dir and make a copy of the env file
    let example_env_file = PathBuf::from("./.env.example");
    let env_file = PathBuf::from("./.env");

    let contents = std::fs::read_to_string(example_env_file)
        .expect("Error: Tried to read .env.example contents but an error occurred");
    std::fs::write(env_file, contents)?;
    logger::add_file_msg(".env");

    logger::project_created_msg(install_config);

    Ok(())
}

fn configure_project(
    query_sync: bool,
    qsync_input_files: Option<Vec<PathBuf>>,
    qsync_output_file: Option<PathBuf>,
    qsync_debug: bool,
    new_service: bool,
) -> Result<()> {
    let current_dir: PathBuf = fs::get_current_working_directory()?;

    if !current_dir.exists() {
        println!("Fatal: the current directory doesn't exist. This shouldn't be possible.");
        return Ok(());
    }

    if !fs::is_rust_project(&current_dir)? {
        // TODO: determine if the current directory is a create-rust-app project.
        println!("Fatal: the current directory is not a rust project.");
        return Ok(());
    }

    // println!("It looks like you ran `create-rust-app` without a [name] argument in a rust project directory.");
    // println!("This functionality has been temporarily disabled in v3 due to our migration to the poem framework. There are plans to support multiple backend frameworks in the future (specifically: actix_web, rocket, axum, warp, and poem).");
    // println!("\nIf you were trying to create a rust app, include the name argument like so:\n\t{}", style("create-rust-app <project_name>").cyan());
    // return Ok(());

    let selection = if query_sync && new_service {
        panic!("--qsync and --new-service are mutually exclusive")
    } else if query_sync {
        Some(0)
    } else if new_service {
        Some(1)
    } else {
        let items = vec![
            "Generate react-query hooks (beta)",
            "Add a model & service (beta)",
            "Cancel",
        ];

        Select::with_theme(&ColorfulTheme::default())
            .items(&items)
            .default(0)
            .interact_on_opt(&Term::stderr())?
    };

    if let Some(index) = selection {
        match index {
            0 => {
                // TODO: maybe obtain this programmatically by parsing the users cargo.toml file?
                logger::message("Which backend framework are you using?");
                logger::message("Use UP/DOWN arrows to navigate and SPACE or ENTER to confirm.");
                let items = vec!["actix_web", "poem"];
                let selection = Select::with_theme(&ColorfulTheme::default())
                    .items(&items)
                    .default(0)
                    .interact_on_opt(&Term::stderr())?;

                match selection {
                    Some(0) => BackendFramework::ActixWeb,
                    Some(1) => panic!("Fatal: this feature is not yet implemented for `poem`"),
                    _ => panic!("Fatal: Unknown backend framework specified."),
                };

                qsync::process(
                    qsync_input_files.unwrap_or_else(|| vec![PathBuf::from("backend/services")]),
                    qsync_output_file
                        .unwrap_or_else(|| PathBuf::from("frontend/src/api.generated.ts")),
                    qsync_debug,
                );
            }
            1 => {
                // Add resource
                let resource_name: String = Input::new()
                    .with_prompt("Resource name")
                    .default("".into())
                    .interact_text()?;

                if resource_name.is_empty() {
                    return Ok(());
                }

                logger::message("Which backend framework are you using?");
                logger::message("Use UP/DOWN arrows to navigate and SPACE or ENTER to confirm.");
                let items = vec!["actix_web", "poem"];
                let selection = Select::with_theme(&ColorfulTheme::default())
                    .items(&items)
                    .default(0)
                    .interact_on_opt(&Term::stderr())?;

                let backend_framework: BackendFramework = match selection {
                    Some(0) => BackendFramework::ActixWeb,
                    Some(1) => BackendFramework::Poem,
                    _ => panic!("Fatal: Unknown backend framework specified."),
                };
                project::create_resource(backend_framework, resource_name.as_ref())?;
                std::process::exit(0);
            }
            2 => return Ok(()),
            _ => {
                logger::error("Not implemented");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
