use clap::ArgGroup;
use clap::ValueHint;
#[allow(unused)]
use clap::{arg, command, value_parser, Arg, ArgAction, Command};
use network::follow::FollowNetwork;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

mod client_utils;
mod listen;
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
        .arg(
            Arg::new("listen mentions")
                .long("listen-mentions")
                .help("Listen for events that mention the client pubkey")
                .value_name("config path")
                .value_hint(ValueHint::FilePath)
                .num_args(1)
        )
        .group(
            ArgGroup::new("Mutually exclusive")
                .args(["run old", "print rank", "separation degrees", "listen mentions"])
                .multiple(false),
        )
        .get_matches();

    if matches.get_one::<bool>("print rank") == Some(&true) {
        print_rank(
            matches.get_one::<String>("user key").unwrap(),
            "put the bot nsec here",
        )
        .await
        .unwrap();
        return Ok(());
    }

    let my_keys = match matches
        .get_one::<String>("connection key")
        .map(|x| x.as_str())
    {
        Some("new") => {
            let keys = Keys::generate();
            eprintln!(
                "generated key: {} {}",
                keys.secret_key().unwrap().to_bech32().unwrap(),
                keys.public_key().to_bech32().unwrap()
            );
            keys
        }
        Some(s) => Keys::parse(s).unwrap(),
        None => Err(nostr_sdk::key::Error::InvalidSecretKey).unwrap(),
    };
    let my_pubkey = my_keys.public_key();
    let (client, user, network) = start_connection(my_keys, my_pubkey).await;

    if let Some(vals) = matches.get_many::<String>("separation degrees") {
        sep_degrees::main(vals.map(|x| x.as_str()), &client, &network).await;
        return Ok(());
    }

    if let Some(config_path) = matches.get_one::<String>("listen mentions") {
        assert!(Path::new(config_path).is_file());
        let client_clone = client.clone();

        async fn second_action(
            event: Event,
            result: Result<(u32, Vec<PublicKey>), sep_degrees::SepDegreeError>,
            client: Arc<Client>,
        ) {
            let message = match result {
                Ok((_, mut path)) => {
                    let mut saudation = "Found Connection:\n\n".to_string();
                    let last = path.pop().unwrap();
                    for pubkey in path.iter() {
                        saudation +=
                            &format!("nostr:{} is mutual with\n", pubkey.to_bech32().unwrap());
                    }
                    if path.is_empty() {
                        saudation += &format!(
                            "nostr:{} is the sole one in this chain",
                            last.to_bech32().unwrap()
                        );
                    } else {
                        saudation += &format!("nostr:{}", last.to_bech32().unwrap());
                    }
                    saudation
                }
                Err(err) => match err {
                    sep_degrees::SepDegreeError::TooFewArguments => {
                        "Too few public keys in request. Use: mention me and then another 2 users!"
                            .to_string()
                    }
                    sep_degrees::SepDegreeError::TooMuchArguments => {
                        "Too much public keys in request. Use: mention me and then another 2 users!"
                            .to_string()
                    }
                    sep_degrees::SepDegreeError::NostrClientError(_error) => {
                        "Nostr client internal error".to_string()
                    }
                    sep_degrees::SepDegreeError::NotFound => {
                        "Connection between users not found".to_string()
                    }
                    sep_degrees::SepDegreeError::MissingContactList(public_key) => format!(
                        "Missing contact list of nostr:{}",
                        public_key.to_bech32().unwrap()
                    ),
                },
            };
            match reply_to_text(&client, &event, &message).await {
                Ok(ok) => println!("Sent event {}", ok.id()),
                Err(err) => eprintln!("Reply error: {err}"),
            };
        }

        listen::listen_mention(
            &client,
            user,
            config_path,
            |x, y| sep_degrees::from_message(x, y, 3),
            (client.clone(), network),
            second_action,
        )
        .await;
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
