/// Algorithms used in the find degrees of separation functionality
use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::client_utils::{self, *};
use crate::map_intersect;
use crate::network::Network;

use nostr_sdk::prelude::*;

pub async fn main(
    vals: impl IntoIterator<Item = &str>,
    client: Arc<Client>,
    network: Arc<Mutex<Network>>,
) {
    let vals = vals
        .into_iter()
        .map(|x| PublicKey::parse(x).expect("Pubkey parse error"))
        .collect_vec();

    let (degree, path) = find_sep_degrees(&client, &network, vals[0], vals[1], 300)
        .await
        .unwrap();

    while !verify_path(&client, &network, path.clone()).await.unwrap() {
        find_sep_degrees(&client, &network, vals[0], vals[1], 300)
            .await
            .unwrap();
    }

    println!("degrees: {degree}");
    let path = path
        .into_iter()
        .map(|x| x.to_bech32().unwrap())
        .collect_vec();
    println!("{:?}", path);
    return;
}

pub async fn verify_path(
    client: &Client,
    network: &Arc<Mutex<Network>>,
    path: Vec<PublicKey>,
) -> Result<bool> {
    eprintln!("Verifying: {:?}", path.iter().map(|x| x.to_bech32()));

    let mut follows = client_utils::get_following_multiple_users_with_timestamp_and_timeout(
        path.clone(),
        &client,
        None,
    )
    .await?;

    let mut net_lock = network.lock().await;
    for (user, (contact_list, _)) in follows.iter() {
        net_lock.update_contact_list(*user, contact_list);
    }

    for (i, j) in (0..path.len()).zip(1..path.len()) {
        if !net_lock.are_users_mutuals(&path[i], &path[j]) {
            return Ok(false);
        }
    }

    Ok(true)
}

pub async fn find_sep_degrees(
    client: &Client,
    network: &Arc<Mutex<Network>>,
    target_1: PublicKey,
    target_2: PublicKey,
    chunk_size: u32,
) -> Result<(u32, Vec<PublicKey>)> {
    // Add targets to network, if they aren't already
    {
        let mut net_lock = network.lock().await;
        net_lock.add_user(target_1);
        net_lock.add_user(target_2);
    }

    // Add targets metadata
    let mut targets_meta = client_utils::get_metadata_users_with_timeout(
        &[target_1, target_2],
        &client,
        Some(Duration::from_secs(30)),
    )
    .await?;

    let target_1_meta = targets_meta
        .clone()
        .remove(&target_1)
        .flatten()
        .expect("Missing metadata");
    let target_2_meta = targets_meta
        .remove(&target_2)
        .flatten()
        .expect("Missing metadata");

    {
        let mut net_lock = network.lock().await;
        net_lock.add_user_metadata(target_1, target_1_meta.0, target_1_meta.1);
        net_lock.add_user_metadata(target_2, target_2_meta.0, target_2_meta.1);
    }

    // Build levels
    let mut mutual_levels_1: Vec<HashMap<PublicKey, PublicKey>> = Vec::new();
    let mut map1 = HashMap::new();
    map1.insert(target_1, target_1);
    mutual_levels_1.push(map1);

    let mut mutual_levels_2: Vec<HashMap<PublicKey, PublicKey>> = Vec::new();
    let mut map2 = HashMap::new();
    map2.insert(target_2, target_2);
    mutual_levels_2.push(map2);

    // Build next level
    let mut follows = client_utils::get_following_multiple_users_with_timestamp_and_timeout(
        vec![target_1, target_2],
        &client,
        None,
    )
    .await?;
    let mut border1 = follows
        .clone()
        .remove(&target_1)
        .expect("Missing contact list")
        .0;
    let mut border2 = follows.remove(&target_2).expect("Missing contact list").0;

    // Advance 1 level at time and check for colisions
    let mut current_distance = 0u32;
    for i in (1..=2).cycle() {
        // Handle finding a match, if any
        let mut intersection = map_intersect::intersection_map(
            mutual_levels_1.last().unwrap(),
            mutual_levels_2.last().unwrap(),
        );

        if let Some((user_match, back1, back2)) = intersection.next() {
            match current_distance {
                0 => {
                    assert_eq!(target_1, target_2);
                    return Ok((0, vec![target_1]));
                }
                1 => {
                    assert!(target_1 == *user_match || target_2 == *user_match);
                    return Ok((1, vec![target_1, target_2]));
                }
                2 => {
                    assert!(target_1 != *user_match || target_2 != *user_match);
                    return Ok((2, vec![target_1, *user_match, target_2]));
                }
                n => {
                    let mut backtrack1 = Vec::new();
                    let mut backtrack2 = Vec::new();
                    {
                        let mut current_back = back1;
                        let mut index = mutual_levels_1.len() - 2;
                        while current_back != &target_1 {
                            backtrack1.push(current_back);
                            current_back = mutual_levels_1[index]
                                .get(current_back)
                                .expect("Missing back in backtrack construction");
                            index -= 1;
                        }
                    }
                    {
                        let mut current_back = back2;
                        let mut index = mutual_levels_2.len() - 2;
                        while current_back != &target_2 {
                            backtrack2.push(current_back);
                            current_back = mutual_levels_2[index]
                                .get(current_back)
                                .expect("Missing back in backtrack construction");
                            index -= 1;
                        }
                    }

                    let mut to_return = vec![target_1];
                    to_return.extend(backtrack1.into_iter().rev());
                    to_return.push(*user_match);
                    to_return.extend(backtrack2.into_iter());
                    to_return.push(target_2);
                    return Ok((n, to_return));
                }
            }
        }

        // Advance levels 1 or 2
        let (mutual_levels_i, border_i) = if i == 1 {
            (&mut mutual_levels_1, &mut border1)
        } else {
            (&mut mutual_levels_2, &mut border2)
        };

        let mut next_map_i: HashMap<PublicKey, PublicKey> = HashMap::new();
        let mut new_border_i: HashSet<PublicKey> = HashSet::new();

        // TODO: chunks
        let mut now = 0;
        let total = border_i.len().div_ceil(chunk_size as usize);
        let border_chunks = border_i.chunks(chunk_size as usize);
        for chunk in border_chunks {
            eprintln!("current: {now}/{total}");

            let mut net_lock = network.lock().await;
            // Filter users that already have their followers in the network
            let chunk = {
                chunk
                    .iter()
                    .filter(|x| !net_lock.does_user_follow(x).is_some())
                    .map(|x| *x)
                    .collect_vec()
            };

            let mut res_contacts =
                client_utils::get_following_multiple_users_with_timestamp_and_timeout(
                    chunk.clone(),
                    &client,
                    None,
                )
                .await?;
            for user in chunk {
                let (contacts, _) = match res_contacts.remove(&user) {
                    Some(s) => s,
                    None => {
                        eprintln!("Didn't find user {user} contact list");
                        continue;
                    }
                };
                net_lock.update_contact_list(user, contacts.iter());
            }
            now += 1;
        }

        for user in &mut *border_i {
            let mut flag_in_next_level = false;
            let mut new_border_i_user = Vec::new();
            let mut net_lock = network.lock().await;

            for follow in net_lock.get_user_contacts(user) {
                if match mutual_levels_i.last() {
                    Some(last_level) => last_level.contains_key(&follow),
                    None => false,
                } {
                    flag_in_next_level = true;
                    next_map_i.insert(*user, *follow);
                } else {
                    if !mutual_levels_i.iter().any(|x| x.contains_key(follow)) {
                        new_border_i_user.push(follow);
                    }
                }
            }
            if flag_in_next_level {
                new_border_i.extend(new_border_i_user);
            }
        }

        mutual_levels_i.push(next_map_i);
        *border_i = new_border_i.into_iter().collect_vec();

        current_distance += 1;

        if current_distance == 7 {
            panic!("Too many levels")
        }
    }

    println!("{:#.4?}", network);
    todo!()
}
