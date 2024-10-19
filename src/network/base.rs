/// Defines a network of users
use itertools::Itertools;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::{HashMap, HashSet};

use nostr_sdk::prelude::*;
use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Following,
}

/// Graph that tracks association between users (follows, etc.)
#[derive(Debug)]
pub struct Network {
    graph: DiGraph<PublicKey, EdgeKind>,
    graph_indices: HashMap<PublicKey, NodeIndex>,
    users_metadata: HashMap<PublicKey, Option<(Metadata, Timestamp)>>,
    added_out_edges: HashMap<PublicKey, Timestamp>,
    all_users: HashSet<PublicKey>,
}

impl Network {
    pub fn new() -> Network {
        Network {
            graph: DiGraph::new(),
            graph_indices: HashMap::new(),
            users_metadata: HashMap::new(),
            added_out_edges: HashMap::new(),
            all_users: HashSet::new(),
        }
    }

    /// Returns (node, false) if user is already in the network
    ///
    /// Otherwise returns (node, true)
    pub fn add_user(&mut self, user: PublicKey) -> (NodeIndex, bool) {
        if let Some(node) = self.graph_indices.get(&user) {
            return (*node, false);
        }

        let val = self.graph.add_node(user);
        self.graph_indices.insert(user, val);
        self.all_users.insert(user);

        (val, true)
    }

    pub fn contains_user(&self, user: &PublicKey) -> bool {
        self.graph_indices.contains_key(&user)
    }

    pub fn add_follow(&mut self, user: PublicKey, follow: PublicKey) -> EdgeIndex {
        let add_user = self.add_user(user).0;
        let add_follow = self.add_user(follow).0;
        self.add_follow_nodes(add_user, add_follow)
    }

    /// Update contact list of user, removing old follows and adding new ones
    pub fn update_contact_list<'a>(
        &mut self,
        user: PublicKey,
        contacts: impl IntoIterator<Item = &'a PublicKey>,
    ) {
        let (node_user, added) = self.add_user(user);
        if !added {
            self.remove_contact_list(user);
        }
        for follow in contacts {
            self.add_follow(user, *follow);
        }
    }

    pub fn remove_contact_list(&mut self, user: PublicKey) {
        let (node_user, added) = self.add_user(user);
        if added {
            return;
        }

        let follows = self
            .graph
            .edges_directed(node_user, Direction::Outgoing)
            .filter(|x| x.weight() == &EdgeKind::Following)
            .map(|x| x.id())
            .collect_vec();
        for follow in follows {
            self.graph.remove_edge(follow);
        }
    }

    pub fn get_following_edge_nodes(
        &self,
        user_node: NodeIndex,
        follow_node: NodeIndex,
    ) -> Option<petgraph::graph::EdgeReference<'_, EdgeKind>> {
        let mut follow_iter = self
            .graph
            .edges_connecting(user_node, follow_node)
            .filter(|x| x.weight() == &EdgeKind::Following);

        follow_iter.next()
    }

    pub fn is_following_nodes(&self, user_node: NodeIndex, follow_node: NodeIndex) -> bool {
        match self.get_following_edge_nodes(user_node, follow_node) {
            Some(_) => true,
            None => false,
        }
    }

    pub fn add_follow_nodes(&mut self, user_node: NodeIndex, follow_node: NodeIndex) -> EdgeIndex {
        match self.get_following_edge_nodes(user_node, follow_node) {
            Some(s) => s.id(),
            None => {
                self.added_out_edges.insert(
                    *self
                        .graph
                        .node_weight(user_node)
                        .expect("Node not in graph"),
                    Timestamp::now(),
                );
                self.graph
                    .update_edge(user_node, follow_node, EdgeKind::Following)
            }
        }
    }

    pub fn does_user_follow(&self, user: &PublicKey) -> Option<Timestamp> {
        self.added_out_edges.get(user).copied()
    }

    pub fn are_users_mutuals(&self, user: &PublicKey, other: &PublicKey) -> bool {
        let node_user = self.pubkey_to_node(user).unwrap();
        let node_other = self.pubkey_to_node(other).unwrap();
        self.is_following_nodes(node_user, node_other)
            && self.is_following_nodes(node_other, node_user)
    }

    pub fn add_user_metadata(
        &mut self,
        user: PublicKey,
        metadata: Metadata,
        timestamp: Timestamp,
    ) -> Option<(Metadata, Timestamp)> {
        self.users_metadata
            .insert(user, Some((metadata, timestamp)))
            .flatten()
    }

    pub fn extend_users_metadata(
        &mut self,
        metadata_iter: impl IntoIterator<Item = (PublicKey, Option<(Metadata, Timestamp)>)>,
    ) {
        self.users_metadata.extend(metadata_iter)
    }

    /// Mark an user as explicitly having no metadata associated
    pub fn add_user_no_metadata(&mut self, user: PublicKey) -> Option<(Metadata, Timestamp)> {
        self.users_metadata.insert(user, None).flatten()
    }

    pub fn get_user_mutuals(&self, user: &PublicKey) -> Vec<NodeIndex> {
        let user_node = match self.graph_indices.get(user) {
            Some(s) => s,
            None => return vec![],
        };
        let outgoing: HashSet<NodeIndex> = self
            .graph
            .edges_directed(*user_node, Direction::Outgoing)
            .filter(|x| x.weight() == &EdgeKind::Following)
            .map(|x| x.target())
            .collect();

        let ingoing: HashSet<NodeIndex> = self
            .graph
            .edges_directed(*user_node, Direction::Incoming)
            .filter(|x| x.weight() == &EdgeKind::Following)
            .map(|x| x.source())
            .collect();

        ingoing.intersection(&outgoing).map(|x| *x).collect_vec()
    }

    pub fn get_user_contacts<'a>(
        &'a self,
        user: &PublicKey,
    ) -> Box<dyn Iterator<Item = &'a PublicKey> + 'a> {
        let user_node = match self.graph_indices.get(user) {
            Some(s) => s,
            // Since it has no items, it cannot access anything with lifetime 'a
            None => return Box::new(std::iter::empty()),
        };
        Box::new(
            self.graph
                .edges_directed(*user_node, Direction::Outgoing)
                .filter(|x| x.weight() == &EdgeKind::Following)
                .map(|x| {
                    self.graph
                        .node_weight(x.target())
                        .expect("Node without weight?!")
                }),
        )
    }

    pub fn node_to_pubkey(&self, node: NodeIndex) -> Option<PublicKey> {
        self.graph.node_weight(node).map(|x| *x)
    }

    pub fn pubkey_to_node(&self, pubkey: &PublicKey) -> Option<NodeIndex> {
        self.graph_indices.get(pubkey).map(|x| *x)
    }

    pub fn get_pubkey_metadata(&self, pubkey: &PublicKey) -> Option<&(Metadata, Timestamp)> {
        match self.users_metadata.get(pubkey) {
            Some(Some(s)) => Some(s),
            Some(None) => None,
            None => None,
        }
    }
}
