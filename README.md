
# Six degrees bot
<p>

This bot finds the connection between users based on the six degrees of separation idea https://en.wikipedia.org/wiki/Six_degrees_of_separation

Two users are considered connected if they are mutuals in the Nostr protocol
</p>

## Usage
<p>
Mention the bot and then another 2 users to find the connection between then in a text note

E.g.: nostr:npub1vvlngyytydfrcdz5jvlx2r5q40ssp0wz4p52p7rvtajllq56mzzs474se7 nostr:[pubkey1] nostr:[pubkey2]

The bot will then reply with the connection of mutuals between the two users, if any
</p>


## Run

    ```
    cargo run -- --connection-key [nsec] --listen-mentions listen.toml
    ```
<p> 
The bot will listen to mentions, then try to find a connection between the other two users mentioned and then reply with the result

</p>
