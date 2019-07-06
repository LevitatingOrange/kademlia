use crate::storage::*;
use crate::util::*;
use byteorder::{BigEndian, ByteOrder};
use bytes::{BufMut, BytesMut};
use serde::{Deserialize, Serialize};
use serde_json;
use std::io;
use tokio::codec::{Decoder, Encoder};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "cmd", content = "data", bound = "K: FindNodeCount")]
pub enum PeerMessage<K: FindNodeCount, S: StorageActor> {
    Ping,
    Pong(KademliaKey),
    FindNode {
        sender_id: KademliaKey,
        id: KademliaKey,
        find_data: bool,
    },
    FoundNodes {
        sender_id: KademliaKey,
        id: KademliaKey,
        nodes: Vec<Connection>,
    },
    FoundKey {
        sender_id: KademliaKey,
        id: KademliaKey,
        data: S::StorageData,
    },
    StoreKey {
        sender_id: KademliaKey,
        key: KademliaKey,
        data: S::StorageData,
    },
    // StoreKeyAnswer {
    //     sender_id: KademliaKey,
    //     key:KademliaKey,
    //     answer: Result<SocketAddr, StorageError>
    // },
    // TODO: check if both needed
    Phantom(std::marker::PhantomData<K>, std::marker::PhantomData<S>),
    // FindNode(Key<N>),
    // FoundNodes(GenericArray<Key<N>, K>),
    //LinearSearchRequest(LinearSearchRequest<N>),
    //LinearSearchResponse(LinearSearchResponse<N>),
    //Disconnect,
}
//#[derive(Default)]
pub struct PeerCodec<K: FindNodeCount, S: StorageActor> {
    p1: std::marker::PhantomData<K>,
    p2: std::marker::PhantomData<S>,
}

impl<K, S> Default for PeerCodec<K, S>
where
    K: FindNodeCount,
    S: StorageActor,
{
    fn default() -> Self {
        Self {
            p1: std::marker::PhantomData::default(),
            p2: std::marker::PhantomData::default(),
        }
    }
}

impl<K, S> Decoder for PeerCodec<K, S>
where
    K: FindNodeCount,
    S: StorageActor,
{
    type Item = PeerMessage<K, S>;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let size = {
            if src.len() < 2 {
                return Ok(None);
            }
            BigEndian::read_u16(src.as_ref()) as usize
        };

        if src.len() >= size + 2 {
            src.split_to(2);
            let buf = src.split_to(size);
            Ok(Some(serde_json::from_slice::<PeerMessage<K, S>>(&buf)?))
        } else {
            Ok(None)
        }
    }
}

impl<K, S> Encoder for PeerCodec<K, S>
where
    K: FindNodeCount,
    S: StorageActor,
{
    type Item = PeerMessage<K, S>;
    type Error = io::Error;

    fn encode(&mut self, msg: PeerMessage<K, S>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let msg = serde_json::to_string(&msg).unwrap();
        let msg_ref: &[u8] = msg.as_ref();

        dst.reserve(msg_ref.len() + 2);
        dst.put_u16_be(msg_ref.len() as u16);
        dst.put(msg_ref);

        Ok(())
    }
}
