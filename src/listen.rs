use async_utility::futures_util::future::join_all;
use clap::ArgGroup;
#[allow(unused)]
use clap::{arg, command, value_parser, Arg, ArgAction, Command};
use itertools::Itertools;
use serde::Deserialize;
use serde::Serialize;
use std::borrow::Borrow;
use std::collections::HashSet;
use std::fs;
use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::join;
use tokio::sync::Mutex;
use tokio::time::interval;

use crate::client_utils::*;
use crate::network::Network;
use crate::user::User;

use nostr_sdk::prelude::*;

use std::env;

#[derive(Debug, Serialize, Deserialize)]
struct Responded(HashSet<EventId>);

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    responded: Responded,
    wait_time_secs: u64,
}

/// Listen for mentions to the key configured in user
///
/// action: Processing of the collected event
///
/// second_action: Action with the result of action, e.g. send a reply
pub async fn listen_mention<T1, T2, S, F>(
    client: &Arc<Client>,
    user: User,
    config_path: &str,
    action: impl Fn(Event, S) -> T1 + Clone + Send + 'static,
    action_args: S,
    second_action: impl Fn(Event, T2, Arc<Client>) -> F + Clone + Send + 'static,
) where
    T1: Future<Output = T2> + Send + 'static,
    T2: std::fmt::Debug + Send + Sync + 'static,
    F: Future<Output = ()> + Send + 'static,
    S: Clone + Send + Sync + 'static,
{
    let config = match fs::read_to_string(config_path) {
        Ok(config_text) => match toml::from_str::<Config>(&config_text) {
            Ok(ok) => ok,
            Err(err) => {
                eprintln!("Config file parse error:\n{}", err);
                return;
            }
        },
        Err(err) => {
            eprintln!("Config file missing: {}", err);
            let config = Config {
                responded: Responded(HashSet::new()),
                wait_time_secs: 100,
            };
            fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
            config
        }
    };
    let config = Arc::new(Mutex::new(config));
    let wait_time = config.lock().await.wait_time_secs;
    let mut delay = interval(Duration::from_secs(wait_time));
    loop {
        delay.tick().await;
        // Listen for mentions
        println!("Looking for new mentions");
        let mentions = {
            let config_lock = config.lock().await;
            match listen_mentions(&client, user.public_key(), None).await {
                Ok(ok) => ok,
                Err(mut err) => {
                    let val;
                    loop {
                        eprintln!("Listen mentions error: {}", err);
                        let res = listen_mentions(&client, user.public_key(), None).await;
                        if let Ok(ok) = res {
                            val = ok;
                            break;
                        } else if let Err(err2) = res {
                            err = err2;
                        }
                    }
                    val
                }
            }
            .filter(|event| !config_lock.responded.0.contains(&event.id))
            .collect_vec()
        };

        let mut tasks = vec![];
        for mention in mentions {
            async fn block<T1, T2, S, F>(
                client: Arc<Client>,
                config: Arc<Mutex<Config>>,
                mention: Event,
                config_path: String,
                action: impl Fn(Event, S) -> T1,
                action_args: S,
                second_action: impl Fn(Event, T2, Arc<Client>) -> F + Clone + Send + 'static,
            ) where
                T1: Future<Output = T2> + Send,
                T2: std::fmt::Debug + Send,
                F: Future + Send + 'static,
            {
                let mention_id = mention.id;

                println!("Read {}", mention_id.to_bech32().unwrap());
                let mut ret = action(mention.clone(), action_args).await;
                println!(
                    "Produced answer: {:?} to {}",
                    ret,
                    mention_id.to_bech32().unwrap()
                );
                second_action(mention, ret, client).await;

                let mut config_lock = config.lock().await;
                config_lock.responded.0.insert(mention_id);
                fs::write(
                    config_path,
                    toml::to_string::<Config>(&config_lock).unwrap(),
                )
                .unwrap();
            }

            tasks.push(tokio::task::spawn(block(
                client.clone(),
                config.clone(),
                mention,
                config_path.to_string(),
                action.clone(),
                action_args.clone(),
                second_action.clone(),
            )));
        }
        let tasks_len = tasks.len();
        if tasks_len == 0 {
            println!("No new mention found. Waiting {} seconds", wait_time);
        }
        for val in join_all(tasks).await {
            match val {
                Ok(()) => (),
                Err(err) => eprintln!("JoinError: {}", err),
            }
        }
    }
}
