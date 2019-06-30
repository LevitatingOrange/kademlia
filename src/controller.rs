use crate::bucket::Bucket;
use crate::codec::PeerMessage;
use crate::util::*;
use actix::prelude::*;
use generic_array::GenericArray;
use log::error;
use std::io;

#[derive(Debug, Message)]
pub enum ControlMessage {
    AddPeer(Connection),
    FindNode(Connection, Key),
    GetNodes(usize),
    ReturnNodes(Vec<Connection>),
}

pub struct Controller<B: BucketListSize<K>, K: FindNodeCount> {
    id: Key,
    buckets: GenericArray<Addr<Bucket<B, K>>, B>,
    baseport: u16,
}

impl<B, K> Controller<B, K>
where
    B: BucketListSize<K>,
    K: FindNodeCount,
{
    // fn get_bucket_for_key(&self, key: &Key<N>) -> &Addr<Bucket<N, B, K>> {
    //     &self.buckets[floor_nearest_power_of_two(xor_metric(&self.id, key))]
    // }
}

impl<B, K> Actor for Controller<B, K>
where
    B: BucketListSize<K>,
    K: FindNodeCount,
{
    type Context = actix::Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // TODO
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        // TODO
        Running::Stop
    }
}

impl<K: FindNodeCount, B: BucketListSize<K>> Handler<ControlMessage> for Controller<B, K> {
    type Result = ();

    fn handle(&mut self, msg: ControlMessage, ctx: &mut Self::Context) {
        match msg {
            ControlMessage::FindNode(peer, other_id) => {
                // TODO handle too few nodes in bucket
                // We compute the baseport
                let d = self.id.distance(&other_id);
                // Send message with request for their nodes
                // recompute port by using -d and adding new distance
                if let Err(err) = self.buckets[d].try_send(ControlMessage::FindNode(peer, other_id))
                {
                    error!("Bucket {} responded with error: {}", d, err);
                }
            }
            _ => error!("Controller got unrecognized message {:?}", msg),
        }
    }
}
