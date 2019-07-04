use crate::util::*;
use byteorder::{BigEndian, ByteOrder};
use bytes::{BufMut, BytesMut};
use serde::{Deserialize, Serialize};
use serde_json;
use std::io;
use tokio::codec::{Decoder, Encoder};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "cmd", content = "data", bound = "K: FindNodeCount")]
pub enum PeerMessage<K: FindNodeCount> {
    Ping,
    Pong(Key),
    FindNode {
        sender_id: Key,
        id: Key,
    },
    FoundNodes {
        sender_id: Key,
        id: Key,
        nodes: Vec<Connection>,
    },
    // TODO: check if both needed
    Phantom(std::marker::PhantomData<K>),
    // FindNode(Key<N>),
    // FoundNodes(GenericArray<Key<N>, K>),
    //LinearSearchRequest(LinearSearchRequest<N>),
    //LinearSearchResponse(LinearSearchResponse<N>),
    //Disconnect,
}
#[derive(Default)]
pub struct PeerCodec<K: FindNodeCount> {
    p2: std::marker::PhantomData<K>,
}

impl<K> Decoder for PeerCodec<K>
where
    K: FindNodeCount,
{
    type Item = PeerMessage<K>;
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
            Ok(Some(serde_json::from_slice::<PeerMessage<K>>(&buf)?))
        } else {
            Ok(None)
        }
    }
}

impl<K> Encoder for PeerCodec<K>
where
    K: FindNodeCount,
{
    type Item = PeerMessage<K>;
    type Error = io::Error;

    fn encode(&mut self, msg: PeerMessage<K>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let msg = serde_json::to_string(&msg).unwrap();
        let msg_ref: &[u8] = msg.as_ref();

        dst.reserve(msg_ref.len() + 2);
        dst.put_u16_be(msg_ref.len() as u16);
        dst.put(msg_ref);

        Ok(())
    }
}
