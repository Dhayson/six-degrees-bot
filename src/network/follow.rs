/// Network that is centered in a particular user, tracking user follows
use async_utility::futures_util::future::try_join_all;
use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use std::usize;
use tokio::sync::Mutex;

use crate::client_utils::*;
use crate::network::*;
use nostr_sdk::prelude::*;

use std::fmt::{self, Display, Formatter};

pub struct FollowNetwork {
    net: Arc<Mutex<Network>>,
    users_distances: HashMap<PublicKey, usize>,
    levels: Vec<HashSet<PublicKey>>,
    client: Arc<Client>,
}

impl fmt::Debug for FollowNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let precision = f.precision().unwrap_or(3);
        match precision {
            1 => f
                .debug_struct("UserNetwork")
                .field("users_distances", &self.users_distances)
                .finish(),
            2 => f
                .debug_struct("UserNetwork")
                .field("net", &self.net)
                .field("users_distances", &self.users_distances)
                .finish(),
            3 => f
                .debug_struct("UserNetwork")
                .field("net", &self.net)
                .field("users_distances", &self.users_distances)
                .field("levels", &self.levels)
                .finish(),
            4 => f
                .debug_struct("UserNetwork")
                .field("net", &self.net)
                .field("users_distances", &self.users_distances)
                .field("levels", &self.levels)
                .finish(),
            5 => f
                .debug_struct("UserNetwork")
                .field("net", &self.net)
                .field("users_distances", &self.users_distances)
                .field("levels", &self.levels)
                .field("client", &self.client)
                .finish(),
            _ => f.debug_struct("UserNetwork").finish(),
        }
    }
}

#[derive(Debug)]
pub enum GetMetadataError {
    LevelNotPresent,
    NostrEventError(Error),
}

impl Display for GetMetadataError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            GetMetadataError::LevelNotPresent => write!(f, "This level is not in the network"),
            GetMetadataError::NostrEventError(error) => write!(f, "{}", error),
        }
    }
}

impl std::error::Error for GetMetadataError {}

impl From<nostr_sdk::client::Error> for GetMetadataError {
    fn from(value: nostr_sdk::client::Error) -> Self {
        Self::NostrEventError(value)
    }
}

#[derive(Debug)]
pub enum RecommendationError {
    NotEnoughLevels,
    InternalGraphError(i32),
}

impl Display for RecommendationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            RecommendationError::NotEnoughLevels => write!(f, "Not Enough Levels"),
            RecommendationError::InternalGraphError(x) => write!(f, "Internal Graph Error {x}"),
        }
    }
}

impl std::error::Error for RecommendationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RankReasons {
    MutualConnections(Vec<PublicKey>),
}

impl FollowNetwork {
    pub async fn new(
        user: crate::user::User,
        client: Arc<Client>,
        net: Arc<Mutex<Network>>,
    ) -> FollowNetwork {
        let user_pubkey = user.public_key();
        {
            let mut net_lock = net.lock().await;
            if !net_lock.contains_user(&user_pubkey) {
                net_lock.add_user(user_pubkey);
                net_lock.add_user_metadata(user_pubkey, user.metadata(), user.last_updated());
            }
        }

        let mut level_zero = HashSet::new();
        level_zero.insert(user_pubkey);

        let mut users_distances = HashMap::new();
        users_distances.insert(user_pubkey, 0);

        FollowNetwork {
            net,
            users_distances,
            levels: vec![level_zero.clone()],
            client,
        }
    }

    pub async fn add_level(&mut self) -> Result<&mut Self> {
        let top_level = self.levels.last().unwrap();
        let current_level = self.levels.len();
        let mut users_following = HashMap::new();

        let chunk_size = 2000;

        // Logging
        eprintln!("add_level: Getting next level on network");
        let size = top_level.len().div_ceil(chunk_size);
        let mut current = 0;
        eprintln!("{current}/{size}");

        let pubkey_chunks = top_level.iter().chunks(chunk_size);

        for chunk in pubkey_chunks.into_iter() {
            let batch = chunk.into_iter().map(|x| *x);
            let client = &self.client;
            let followings = get_following_multiple_users_with_timestamp_and_timeout(
                batch,
                client,
                Some(Duration::from_secs(20)),
            )
            .await?;

            users_following.extend(followings);

            // Logging
            current += 1;
            eprintln!("{current}/{size}");
        }

        // Add to new users in next_level and to weighs
        let mut next_level = HashSet::new();
        for (_, (followings, _)) in &users_following {
            // Make sure to add newly found users
            let follow_iter = followings
                .iter()
                .filter(|x| !self.levels.iter().any(|y| y.contains(x)));
            next_level.extend(follow_iter.clone());
            for following in follow_iter {
                self.users_distances.insert(*following, current_level);
            }
        }
        self.levels.push(next_level.clone());

        {
            let mut net_lock = self.net.lock().await;
            // Add to graph and node map
            for (user, (followings, _)) in &users_following {
                let node_user = net_lock.pubkey_to_node(user).unwrap().clone();
                for following in followings {
                    match net_lock.pubkey_to_node(following) {
                        Some(node_mutual) => {
                            net_lock.add_follow_nodes(node_user, node_mutual);

                            // Não precisa atualizar o map dos índices
                        }
                        None => {
                            let node_mutual = net_lock.add_user(*following).0;
                            net_lock.add_follow_nodes(node_user, node_mutual);
                        }
                    };
                }
            }
        }

        eprintln!("add_level_mutual: Finished");
        Ok(self)
    }

    pub async fn add_metadata(&mut self, level: usize) -> Result<(), GetMetadataError> {
        let chunk_size = 2000;

        // Logging
        eprintln!("add_metadata: Getting metadata");

        match self.levels.get(level) {
            Some(lvl) => {
                // Logging
                let size = lvl.len().div_ceil(chunk_size);
                let mut current = 0;
                eprintln!("{current}/{size}");

                let pubkey_chunks = lvl.iter().chunks(chunk_size);
                for chunk in pubkey_chunks.into_iter() {
                    let batch: Vec<PublicKey> = chunk.into_iter().map(|x| *x).collect();
                    let metadata = get_metadata_users_with_timeout(
                        &batch,
                        &self.client,
                        Some(Duration::from_secs(20)),
                    )
                    .await?;

                    self.net.lock().await.extend_users_metadata(metadata);

                    // Logging
                    current += 1;
                    eprintln!("{current}/{size}");
                }

                Ok(())
            }
            None => Err(GetMetadataError::LevelNotPresent),
        }
    }

    #[deprecated]
    pub async fn add_level_mutual(&mut self) -> Result<&mut Self> {
        let top_level = self.levels.last().unwrap();
        let current_level = self.levels.len();
        let mut mutual_futures = vec![];

        // NOTA: isso pode criar centenas ou milhares de threads e, desse modo, de requests
        eprintln!("add_level_mutual: Getting next level on network");
        for pubkey in top_level {
            let client = self.client.clone();
            mutual_futures.push(async move {
                #[allow(deprecated)]
                let x = match get_mutuals_user(*pubkey, &client).await {
                    Ok(ok) => Ok((*pubkey, ok)),
                    Err(err) => Err(err),
                };
                x
            });
        }
        let mut mutuals_of_users = vec![];
        let batches: Vec<Vec<_>> = mutual_futures
            .into_iter()
            .chunks(100)
            .into_iter()
            .map(|x| x.collect())
            .collect();

        let size = batches.len();
        let mut current = 0;
        eprintln!("{current}/{size}");
        for batch in batches {
            mutuals_of_users.append(&mut try_join_all(batch).await?);
            current += 1;
            eprintln!("{current}/{size}");
        }

        // Add to new users in next_level and to weighs
        let mut next_level = HashSet::new();
        for (_, mutuals) in &mutuals_of_users {
            // Make sure to add newly found users
            let mutual_iter = mutuals
                .iter()
                .filter(|x| !self.levels.iter().any(|y| y.contains(x)));
            next_level.extend(mutual_iter.clone());
            for mutual in mutual_iter {
                self.users_distances.insert(*mutual, current_level);
            }
        }
        self.levels.push(next_level.clone());

        {
            let mut net_lock = self.net.lock().await;
            // Add to graph and node map
            for (user, mutuals) in &mutuals_of_users {
                let node_user = net_lock.pubkey_to_node(user).unwrap().clone();
                for mutual in mutuals {
                    match net_lock.pubkey_to_node(mutual) {
                        Some(node_mutual) => {
                            net_lock.add_follow_nodes(node_user, node_mutual);
                            net_lock.add_follow_nodes(node_mutual, node_user);

                            // Não precisa atualizar o map dos índices
                        }
                        None => {
                            let node_mutual = net_lock.add_user(*mutual).0;
                            net_lock.add_follow_nodes(node_user, node_mutual);
                            net_lock.add_follow_nodes(node_mutual, node_user);
                        }
                    };
                }
            }

            eprintln!("add_level_mutual: Getting metadata");
            // Add new users metadata
            let next_level: Vec<PublicKey> = next_level.drain().collect();
            let metadata_mutuals = get_metadata_users_fake(&next_level, &self.client).await?;
            net_lock.extend_users_metadata(metadata_mutuals.into_iter());
        }

        eprintln!("add_level_mutual: Finished");
        Ok(self)
    }

    /// Rank users based on their connectivity
    /// Focuses on users in level 2, i.e. follows/mutuals of follows
    pub async fn generate_user_ranks(
        &self,
    ) -> Result<Vec<(PublicKey, i32, Vec<RankReasons>)>, RecommendationError> {
        if self.levels.len() <= 2 {
            return Err(RecommendationError::NotEnoughLevels);
        }
        let mut users_ranks = HashMap::new();
        for user in self.levels.get(2).unwrap() {
            let mut rank = 0;
            let mut rank_reasons = vec![];
            let net_lock = self.net.lock().await;
            let user_mutuals_nodes = net_lock.get_user_mutuals(user);
            let user_mutuals = user_mutuals_nodes
                .iter()
                .map(|x| net_lock.node_to_pubkey(*x));

            // Find mutuals
            let mut mutual_reasons = vec![];
            for user_mutual in user_mutuals {
                if let Some(user_mutual) = user_mutual {
                    if self.levels.get(1).unwrap().contains(&user_mutual) {
                        rank += 10;
                        mutual_reasons.push(user_mutual);
                    }
                    // else do nothing
                }
            }
            rank_reasons.push(RankReasons::MutualConnections(mutual_reasons));

            users_ranks.insert(user, (rank, rank_reasons));
        }

        let mut vec: Vec<(PublicKey, i32, Vec<RankReasons>)> = users_ranks
            .into_iter()
            .map(|(x, (y, z))| (*x, y, z))
            .collect();
        vec.sort_by_cached_key(|(_, y, _)| *y);
        return Ok(vec);
    }
}
