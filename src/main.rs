use clap::ArgGroup;
#[allow(unused)]
use clap::{arg, command, value_parser, Arg, ArgAction, Command};
use network::follow::FollowNetwork;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

mod client_utils;
mod map_intersect;
mod network;
mod sep_degrees;
mod user;

use client_utils::*;
use network::Network;
use user::User;

use nostr_sdk::prelude::*;

use std::env;

async fn start_connection(
    con_keys: Keys,
    my_pubkey: PublicKey,
) -> (
    Arc<nostr_sdk::Client>,
    User,
    Arc<tokio::sync::Mutex<Network>>,
) {
    let client = Arc::new(build_client(&con_keys).await);
    let user = User::new(my_pubkey, &client)
        .await
        .expect("User creation error");
    let network = Arc::new(Mutex::new(Network::new()));
    (client, user, network)
}

#[tokio::main]
async fn main() -> Result<()> {
    env::set_var("RUST_BACKTRACE", "0");

    let matches = command!()
        .arg(
            Arg::new("print rank")
                .long("print-rank")
                .action(ArgAction::SetTrue)
                .help("Pretty print recommendations rank"),
        )
        .arg(
            Arg::new("connection key")
                .long("connection-key")
                .help("Set connection authentication key")
                .required_unless_present_any(["print rank", "run old"]),
        )
        .arg(
            Arg::new("user key")
                .long("user-key")
                .help("User Nostr npub or nsec"),
        )
        .group(
            ArgGroup::new("requires key")
                .arg("print rank")
                .requires("user key"),
        )
        .arg(
            Arg::new("run old")
                .long("run-old")
                .action(ArgAction::SetTrue)
                .hide(true),
        )
        .arg(
            Arg::new("separation degrees")
                .long("sep-degree")
                .help("Find degree of separation between two users. Also outputs which users make the shortest path, if found")
                .value_name("npub")
                .num_args(2),
        )
        .group(
            ArgGroup::new("Mutually exclusive")
                .args(["run old", "print rank", "separation degrees"])
                .multiple(false),
        )
        .get_matches();

    let default_keys = match matches.get_one::<String>("connection key") {
        Some(s) => Keys::parse(s),
        None => Err(nostr_sdk::key::Error::InvalidSecretKey),
    };

    if matches.get_one::<bool>("print rank") == Some(&true) {
        print_rank(
            matches.get_one::<String>("user key").unwrap(),
            "put the bot nsec here",
        )
        .await
        .unwrap();
        return Ok(());
    }

    let my_keys = default_keys.unwrap();
    let my_pubkey = my_keys.public_key();
    let (client, user, network) = start_connection(my_keys, my_pubkey).await;

    if let Some(vals) = matches.get_many::<String>("separation degrees") {
        sep_degrees::main(vals.map(|x| x.as_str()), client, network).await;
        return Ok(());
    }

    println!("No arguments were given");
    Ok(())
}

async fn print_rank(key: &str, nsec: &str) -> Result<()> {
    // It's ok if my_keys doesn't match my_pubkey, because the 1st is used in the client and the 2nd is used in
    // the program's logic. Events will only be signed with the bot key but they aren't here so it doesn't matter
    let (my_keys, my_pubkey) = match Keys::parse(key) {
        Ok(ok) => {
            let pubkey = ok.public_key();
            (ok, pubkey)
        }
        Err(_err) => (
            Keys::parse(nsec).unwrap(),
            PublicKey::parse(key).expect("Key parse error"),
        ),
    };

    let (client, user, network) = start_connection(my_keys, my_pubkey).await;
    let mut user_network = FollowNetwork::new(user.clone(), client.clone(), network.clone()).await;

    user_network.add_level().await?;
    user_network.add_metadata(1).await?;
    user_network.add_level().await?;
    user_network.add_metadata(2).await?;
    user_network.add_level().await?;

    let res = user_network.generate_user_ranks().await?;
    for (pubkey, rank, reasons) in res.iter().rev() {
        let net_lock = network.lock().await;
        println!(
            "{} | {} | rank: {}",
            match net_lock.get_pubkey_metadata(pubkey) {
                Some((m, _)) => match &m.name {
                    Some(n) => n,
                    None => match &m.display_name {
                        Some(dn) => dn,
                        None => "None",
                    },
                },
                None => "None",
            },
            pubkey.to_bech32()?,
            rank
        );

        for reason in reasons {
            match reason {
                network::follow::RankReasons::MutualConnections(vec) => {
                    for pubkey2 in vec {
                        println!(
                            "- {:?} | {}",
                            match net_lock.get_pubkey_metadata(&pubkey2) {
                                Some((m, _)) => m.name.clone(),
                                None => None,
                            },
                            pubkey2.to_bech32()?,
                        );
                    }
                }
            }
        }
    }

    println!("{:#.4?}", user_network);

    Ok(())
}
