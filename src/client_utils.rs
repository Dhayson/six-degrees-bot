/// Useful function to interact with client API
use itertools::Itertools;
use nostr_sdk::prelude::*;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

pub async fn build_client(keys: impl Into<NostrSigner>) -> Client {
    // Configure client
    let connection: Connection = Connection::new();
    let opts = Options::new().connection(connection);

    // Create new client with custom options.
    // Use `Client::new(signer)` to construct the client with a custom signer and default options
    // or `Client::default()` to create one without signer and with default options.
    let client = Client::with_opts(keys, opts);

    // Add relays
    // TODO: configure file to select relays
    client
        .add_relay("wss://relay.damus.io")
        .await
        .expect("Relay parse error");
    client
        .add_relay("wss://relay.primal.net")
        .await
        .expect("Relay parse error");
    client
        .add_relay("wss://nos.lol")
        .await
        .expect("Relay parse error");
    client
        .add_relay("wss://strfry.iris.to")
        .await
        .expect("Relay parse error");
    // client.add_relay("wss://purplepag.es").await?;
    // client
    //     .add_relay("wss://lnbits.aruku.kro.kr/nostrrelay/private")
    //     .await?;

    // Connect to relays
    client.connect().await;

    client
}

pub async fn listen_mentions(
    client: &Client,
    pubkey: PublicKey,
    timeout: Option<Duration>,
) -> Result<impl Iterator<Item = Event>, Error> {
    let filter_mention = Filter::new().pubkey(pubkey).kind(Kind::TextNote);
    let mention_mark = "nostr:".to_string() + &pubkey.to_bech32().unwrap();
    let events = client
        .get_events_of(vec![filter_mention], EventSource::relays(timeout))
        .await?;

    // Filter events that mention the pubkey directly
    let events = events
        .into_iter()
        .filter(move |event| event.content.contains(&mention_mark));

    Ok(events)
}

use regex::Regex;
pub fn find_pubkeys_in_message(content: &str) -> Vec<PublicKey> {
    let pubkey_regex: Regex = Regex::new(r"nostr:npub[a-zA-Z0-9]*").unwrap();
    pubkey_regex
        .find_iter(content)
        .filter_map(|x| PublicKey::from_nostr_uri(x.as_str()).ok())
        .collect_vec()
}

/// Get the old event tags and build the tags of reply
pub fn map_event_tags_to_reply(event: &Event) -> Vec<Tag> {
    if event
        .tags
        .iter()
        .all(|tag| !(tag.is_root() || tag.is_reply()))
    {
        // Event doesn't reference other events
        let event_ref = Tag::custom(
            TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
            [&event.id.to_hex(), "", "root"],
        );
        let author_ref = Tag::public_key(event.pubkey);
        let mut to_return = vec![event_ref, author_ref];
        return to_return;
    } else {
        let event_ref = Tag::custom(
            TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
            [&event.id.to_hex(), "", "reply"],
        );
        let author_ref = Tag::public_key(event.pubkey);
        let mut to_return = vec![event_ref, author_ref];
        to_return.extend(event.tags.clone().into_iter().filter_map(|x| {
            if x.is_root()
                || x.kind() == TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::P))
            {
                Some(x.clone())
            } else {
                None
            }
        }));
        return to_return;
    }
}

pub async fn reply_to_text(
    client: &Client,
    event: &Event,
    content: &str,
) -> Result<Output<EventId>, Error> {
    client
        .publish_text_note(content, map_event_tags_to_reply(&event))
        .await
}

pub async fn send_text(my_keys: &Keys, client: &Client, content: &str) -> Result<(), Error> {
    // New text note
    let event: Event = EventBuilder::text_note(content, [])
        .custom_created_at(Timestamp::from(1662669349))
        .to_event(my_keys)?;

    client.send_event(event).await?;

    Ok(())
}

pub async fn send_text_dummy(my_keys: &Keys, client: &Client, content: &str) -> Result<(), Error> {
    // New text note but with wrong kind
    let event = EventBuilder::new(Kind::Custom(50001), content, []).to_event(my_keys)?;
    client.send_event(event).await?;
    Ok(())
}

pub async fn get_following_multiple_users_with_timestamp_and_timeout(
    users: impl IntoIterator<Item = PublicKey>,
    client: &Client,
    timeout: Option<Duration>,
) -> Result<HashMap<PublicKey, (Vec<PublicKey>, Timestamp)>, Error> {
    let filter_following = Filter::new().authors(users).kind(Kind::ContactList);
    let events = client
        .get_events_of(vec![filter_following], EventSource::relays(timeout))
        .await?;

    let mut map = HashMap::new();

    if events.len() == 0 {
        return Ok(map);
    }

    // Get all events associated with an user
    let mut user_events = HashMap::with_capacity(events.len());

    for event3 in events {
        let user = event3.author();
        match user_events.remove(&user) {
            None => user_events.insert(user, vec![event3]),
            Some(mut s) => {
                s.push(event3);
                user_events.insert(user, s)
            }
        };
    }

    // Filter for newest event
    let newest_events = user_events.iter().map(|(pubkey, events)| {
        (
            pubkey,
            events
                .iter()
                .max_by(|x, y| x.created_at().cmp(&y.created_at()))
                .unwrap(),
        )
    });

    // Map event3 into list of pubkeys
    for (pubkey, event3) in newest_events {
        let created_at = event3.created_at();

        let tags_3 = event3.tags();
        let mut pubkeys = vec![];
        for tag in tags_3 {
            match tag.as_vec() {
                [p, pubkey] if p == "p" => match PublicKey::parse(pubkey) {
                    Ok(ok) => pubkeys.push(ok),
                    Err(err) => eprintln!("Public key {pubkey} parse error: {err}"),
                },
                _ => (),
            }
        }
        map.insert(*pubkey, (pubkeys, created_at));
    }

    Ok(map)
}

pub async fn get_following_user_with_timestamp_and_timeout(
    pubkey: PublicKey,
    client: &Client,
    timeout: Option<Duration>,
) -> Result<Option<(Vec<PublicKey>, Timestamp)>, Error> {
    let filter_following = Filter::new().author(pubkey).kind(Kind::ContactList);
    let events = client
        .get_events_of(vec![filter_following], EventSource::relays(timeout))
        .await?;

    if events.len() == 0 {
        return Ok(None);
    }

    let event_3;
    if events.len() > 1 {
        // eprintln!("Multiple kind 3 events received:\n{events:#?}");
        event_3 = events
            .iter()
            .max_by(|x, y| x.created_at().cmp(&y.created_at()))
            .unwrap();
    } else {
        event_3 = &events.get(0).unwrap();
    }

    let created_at = event_3.created_at();

    let tags_3 = event_3.tags();
    let mut pubkeys = vec![];
    for tag in tags_3 {
        match tag.as_vec() {
            [p, pubkey] if p == "p" => match PublicKey::parse(pubkey) {
                Ok(ok) => pubkeys.push(ok),
                Err(err) => (), //eprintln!("Public key {pubkey} parse error: {err}"),
            },
            _ => (),
        }
    }
    Ok(Some((pubkeys, created_at)))
}

pub async fn get_following_user_with_timeout(
    pubkey: PublicKey,
    client: &Client,
    timeout: Option<Duration>,
) -> Result<Option<Vec<PublicKey>>, Error> {
    match get_following_user_with_timestamp_and_timeout(pubkey, client, timeout).await {
        Ok(Some((s, _))) => Ok(Some(s)),
        Ok(None) => Ok(None),
        Err(err) => Err(err),
    }
}

pub async fn get_following_user(
    pubkey: PublicKey,
    client: &Client,
) -> Result<Option<Vec<PublicKey>>, Error> {
    get_following_user_with_timeout(pubkey, client, None).await
}

/// Not recommended
#[deprecated]
pub async fn get_followers_user(
    pubkey: PublicKey,
    client: &Client,
) -> Result<Vec<PublicKey>, Error> {
    let filter_followers = Filter::new().kind(Kind::ContactList).pubkey(pubkey);
    let timeout = Some(Duration::from_secs(30));
    let events = client
        .get_events_of(vec![filter_followers], EventSource::relays(timeout))
        .await?;

    let users: Vec<PublicKey> = events.iter().map(|event| event.author()).unique().collect();
    Ok(users)
}

#[deprecated]
pub async fn get_mutuals_user(pubkey: PublicKey, client: &Client) -> Result<Vec<PublicKey>, Error> {
    let following = get_following_user(pubkey, &client).await?.unwrap_or(vec![]);
    #[allow(deprecated)]
    let followers = get_followers_user(pubkey, &client).await?;

    let set_following: HashSet<PublicKey> = following.into_iter().collect();
    let set_followers: HashSet<PublicKey> = followers.into_iter().collect();

    let mutuals: Vec<PublicKey> = set_followers
        .intersection(&set_following)
        .cloned()
        .collect_vec();

    Ok(mutuals)
}

pub async fn get_metadata_users(
    pubkeys: &[PublicKey],
    client: &Client,
) -> Result<HashMap<PublicKey, Option<(Metadata, Timestamp)>>, Error> {
    get_metadata_users_with_timeout(pubkeys, client, None).await
}

pub async fn get_metadata_users_fake(
    _pubkeys: &[PublicKey],
    _client: &Client,
) -> Result<HashMap<PublicKey, Option<(Metadata, Timestamp)>>, Error> {
    Ok(HashMap::new())
}

pub async fn get_metadata_users_with_timeout(
    pubkeys: &[PublicKey],
    client: &Client,
    timeout: Option<Duration>,
) -> Result<HashMap<PublicKey, Option<(Metadata, Timestamp)>>, Error> {
    let user_metadata = Filter::new().authors(pubkeys.to_vec()).kind(Kind::Metadata);
    let events = client
        .get_events_of(vec![user_metadata], EventSource::relays(timeout))
        .await?;
    // eprintln!("{:?}", events);
    let mut map_pubkey_meta = HashMap::with_capacity(pubkeys.len());
    for event in events {
        let pubkey = event.pubkey;
        let created_at = event.created_at();
        let metadata = match Metadata::from_json(event.content()) {
            Ok(meta) => meta,
            Err(err) => {
                eprintln!("Metadata from {pubkey} parse error: {err}");
                continue;
            }
        };
        match map_pubkey_meta.get(&pubkey) {
            Some(Some((m, t))) => {
                // Considera eventos mais recentes prioritariamente
                if t > &created_at {
                    // eprintln!("Multiple metadata received from pubkey {pubkey}:\n{metadata:#?}\ncreated at: {created_at:#?}\n\n");
                } else {
                    // eprintln!("Multiple metadata received from pubkey {pubkey}:\n{m:#?}\ncreated at: {t:#?}\n\n");
                    map_pubkey_meta.insert(pubkey, Some((metadata, created_at)));
                }
            }
            Some(None) => unreachable!(),
            None => _ = map_pubkey_meta.insert(pubkey, Some((metadata, created_at))),
        };
    }
    for pubkey in pubkeys {
        match map_pubkey_meta.get(&pubkey) {
            None => {
                _ = {
                    map_pubkey_meta.insert(*pubkey, None);
                    eprintln!("No metadata from pubkey {}", pubkey.to_bech32().unwrap());
                }
            }
            Some(_m) => (), //eprintln!("Ye metadata from pubkey {}", pubkey.to_bech32().unwrap()),
        };
    }

    Ok(map_pubkey_meta)
}
