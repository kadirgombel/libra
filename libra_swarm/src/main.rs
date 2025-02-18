// Copyright (c) The Libra Core Contributors
// SPDX-License-Identifier: Apache-2.0

use config::config::{NodeConfig, RoleType};
use libra_swarm::{client, swarm::LibraSwarm};
use std::path::Path;
use structopt::StructOpt;
use tools::tempdir::TempPath;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "libra_swarm",
    author = "Libra",
    about = "Libra swarm to start local nodes"
)]
struct Args {
    /// Number of nodes to start (1 by default)
    #[structopt(short = "n", long = "num_nodes")]
    pub num_nodes: Option<usize>,
    /// Enable logging
    #[structopt(short = "l", long = "enable_logging")]
    pub enable_logging: bool,
    /// Start client
    #[structopt(short = "s", long = "start_client")]
    pub start_client: bool,
    /// Directory used by launch_swarm to output LibraNodes' config files, logs, libradb, etc,
    /// such that user can still inspect them after exit.
    /// If unspecified, a temporary dir will be used and auto deleted.
    #[structopt(short = "c", long = "config_dir")]
    pub config_dir: Option<String>,
    /// If greater than 0, starts a full node swarm connected to the first node in the validator
    /// swarm.
    #[structopt(short = "f", long = "num_full_nodes")]
    pub num_full_nodes: Option<usize>,
}

fn main() {
    let args = Args::from_args();
    let num_nodes = args.num_nodes.unwrap_or(1);
    let num_full_nodes = args.num_full_nodes.unwrap_or(0);
    let (faucet_account_keypair, faucet_key_file_path, _temp_dir) =
        generate_keypair::load_faucet_key_or_create_default(None);

    println!(
        "Faucet account created in (loaded from) file {:?}",
        faucet_key_file_path
    );

    let mut validator_swarm = LibraSwarm::configure_swarm(
        num_nodes,
        RoleType::Validator,
        faucet_account_keypair.clone(),
        args.config_dir.clone(),
        None, /* template_path */
        None, /* upstream_config_dir */
    )
    .expect("Failed to configure validator swarm");

    let mut full_node_swarm = if num_full_nodes > 0 {
        Some(
            LibraSwarm::configure_swarm(
                num_full_nodes,
                RoleType::FullNode,
                faucet_account_keypair,
                None, /* config dir */
                None, /* template_path */
                Some(String::from(
                    validator_swarm
                        .dir
                        .as_ref()
                        .join("0")
                        .to_str()
                        .expect("Failed to convert std::fs::Path to String"),
                )),
            )
            .expect("Failed to configure full node swarm"),
        )
    } else {
        None
    };
    validator_swarm
        .launch_attempt(!args.enable_logging)
        .expect("Failed to launch validator swarm");
    if let Some(ref mut swarm) = full_node_swarm {
        swarm
            .launch_attempt(!args.enable_logging)
            .expect("Failed to launch full node swarm");
    }

    let validator_config = NodeConfig::load(&validator_swarm.config.configs[0]).unwrap();
    let validator_set_file = validator_swarm
        .dir
        .as_ref()
        .join("0")
        .join(&validator_config.consensus.consensus_peers_file);
    println!("To run the Libra CLI client in a separate process and connect to the validator nodes you just spawned, use this command:");
    println!(
        "\tcargo run --bin client -- -a localhost -p {} -s {:?} -m {:?}",
        validator_config
            .admission_control
            .admission_control_service_port,
        validator_set_file,
        faucet_key_file_path,
    );
    if let Some(ref swarm) = full_node_swarm {
        let full_node_config = NodeConfig::load(&swarm.config.configs[0]).unwrap();
        println!("To connect to the full nodes you just spawned, use this command:");
        println!(
            "\tcargo run --bin client -- -a localhost -p {} -s {:?} -m {:?}",
            full_node_config
                .admission_control
                .admission_control_service_port,
            validator_set_file,
            faucet_key_file_path,
        );
    }

    let tmp_mnemonic_file = TempPath::new();
    tmp_mnemonic_file.create_as_file().unwrap();
    if args.start_client {
        let client = client::InteractiveClient::new_with_inherit_io(
            validator_swarm.get_ac_port(0),
            Path::new(&faucet_key_file_path),
            &tmp_mnemonic_file.path(),
            validator_set_file.into_os_string().into_string().unwrap(),
        );
        println!("Loading client...");
        let _output = client.output().expect("Failed to wait on child");
        println!("Exit client.");
    } else {
        // Explicitly capture CTRL-C to drop LibraSwarm.
        let (tx, rx) = std::sync::mpsc::channel();
        ctrlc::set_handler(move || {
            tx.send(())
                .expect("failed to send unit when handling CTRL-C");
        })
        .expect("failed to set CTRL-C handler");
        println!("CTRL-C to exit.");
        rx.recv()
            .expect("failed to receive unit when handling CTRL-C");
    }
    if let Some(dir) = &args.config_dir {
        println!("Please manually cleanup {:?} after inspection", dir);
    }
    println!("Exit libra_swarm.");
}
