use crate::codec::{PeerCodec, PeerMessage};
use crate::controller::ControlMessage;
use crate::controller::Controller;
use crate::util::*;
use actix::io::FramedWrite;
use actix::prelude::*;
use generic_array::typenum::{NonZero, Unsigned};
use generic_array::GenericArray;
use log::{error, info, warn};
use std::collections::HashMap;
use std::io;
use std::mem::replace;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::WriteHalf;
use tokio::net::{UdpFramed, UdpSocket};
use tokio::prelude::stream::SplitSink;
use tokio::prelude::Sink;

pub struct Bucket<B: BucketListSize<K>, K: FindNodeCount> {
    own_id: Key,
    controller: Addr<Controller<B, K>>,
    bucket_index: usize,
    bucket_size: usize,
    bucket: Vec<Connection>,
    queue: Vec<Connection>,
    ping_timeout: Duration,
    ping_timeout_handles: HashMap<Key, SpawnHandle>,
    // write part of the udp stream wrapped so that Messages are deserialized automatically
    framed_write_stream: SplitSink<UdpFramed<PeerCodec<K>>>,
}

impl<B: BucketListSize<K>, K: FindNodeCount> Bucket<B, K> {
    fn new(
        own_id: Key,
        controller: Addr<Controller<B, K>>,
        bucket_index: usize,
        bucket_size: usize,
        ping_timeout: Duration,
        framed_write_stream: SplitSink<UdpFramed<PeerCodec<K>>>,
    ) -> Self {
        Bucket {
            own_id,
            controller,
            bucket_index,
            bucket_size,
            bucket: Vec::with_capacity(bucket_size),
            queue: Vec::new(),
            ping_timeout,
            ping_timeout_handles: HashMap::new(),
            framed_write_stream,
        }
    }

    fn got_contact(&mut self, ctx: &mut Context<Bucket<B, K>>, peer: Connection) {
        if let Some((i, _)) = self.bucket.iter().enumerate().find(|(_, c)| &peer == *c) {
            let d = self.bucket.remove(i);
            self.bucket.push(d);
        } else if self.bucket.len() < self.bucket_size {
            self.bucket.push(peer);
        } else {
            self.queue.push(peer);
            // TODO: what happens if two contacts happen in-between the pin
            if let Err(err) = (&mut self.framed_write_stream)
                .send((PeerMessage::Ping, self.bucket.last().unwrap().address))
                .wait()
            {
                error!("Sink error: {}, stopping actor!", err);
                ctx.stop();
            } else {
                let peer_id = peer.id.clone();
                self.ping_timeout_handles.insert(peer.id, ctx.run_later(self.ping_timeout, move |act: &mut Bucket<B, K>, &mut _| {
                    if let Some(index) = act.bucket.iter().position(|p| p.id == peer_id) {
                        // TODO remove and insert same place
                        if let Some(new_peer) = act.queue.pop() {
                            let old = replace(&mut act.bucket[index], new_peer);
                            info!("Ping timed out to {}, using cached peer", old);
                        } else {
                            let old = act.bucket.remove(index);
                            info!("Ping timed out to {}, cannot use peer from cache as the cache empty!", old);
                        }
                    } 
                    else {
                        if let Some(new_peer) = act.queue.pop() {
                        info!("Peer not found in bucket, moving cached peer to top");
                            act.bucket.push(new_peer);
                        } else {
                            info!("Bucket was empty, Cache was empty");
                        }
                    }
                    act.ping_timeout_handles.remove(&peer_id);
                }));
            }
        }
    }
}

impl<K: FindNodeCount, B: BucketListSize<K>> Actor for Bucket<B, K> {
    type Context = actix::Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // TODO
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        // TODO
        Running::Stop
    }
}

impl<K: FindNodeCount, B: BucketListSize<K>> Handler<ControlMessage> for Bucket<B, K> {
    type Result = ();

    fn handle(&mut self, msg: ControlMessage, ctx: &mut Self::Context) {
        match msg {
            ControlMessage::AddPeer(peer) => {
                self.got_contact(ctx, peer);
            }
            ControlMessage::FindNode(peer, id) => {
                let mut nodes = self.bucket.to_vec();
                nodes.truncate(K::to_usize());
                for node in &mut nodes {
                    let old_port = node.address.port();
                    node.address.set_port(
                        old_port - self.own_id.distance(&node.id) as u16
                            + id.distance(&node.id) as u16,
                    );
                }
                if let Err(err) = (&mut self.framed_write_stream)
                    .send((PeerMessage::FoundNodes(nodes), peer.address))
                    .wait()
                {
                    error!("Sink error: {}, stopping actor!", err);
                    ctx.stop();
                }
            }
            _ => error!("Bucket: unrecognized message {:?}", msg),
        }
    }
}

impl<B, K> StreamHandler<PeerMessage<K>, io::Error> for Bucket<B, K>
where
    B: BucketListSize<K>,
    K: FindNodeCount,
{
    fn handle(&mut self, msg: PeerMessage<K>, ctx: &mut Self::Context) {
        // Got pong from peer, so we don't replace it with the first peer in the cache.
        match msg {
            PeerMessage::Pong(id) => {
                if let Some(handle) = self.ping_timeout_handles.get(&id) {
                    ctx.cancel_future(*handle);
                }
            }
            // TODO: handle less than k receivers
            PeerMessage::FindNode {
                sender_id,
                id,
                sender_addr,
            } => {
                let conn = Connection::new(sender_addr, sender_id);
                // let mut nodes = (&self.bucket).to_vec();
                // nodes.truncate(K::to_usize());
                // TODO: Blocking ok here? Error handling?
                // if let Err(err) = (&mut self.framed_write_stream)
                //     .send((PeerMessage::FoundNodes(nodes), sender_addr))
                //     .wait()
                if let Err(err) = self.controller.try_send(ControlMessage::FindNode(conn, id)) {
                    error!("Controller error: {}, stopping actor!", err);
                    ctx.stop();
                }
                // Every node sends its request directly to the correct bucket.
                // Therefor, the central controller does not need to be
                // involved.
                self.got_contact(ctx, conn);
            }
            //PeerMessage::FoundNodes(nodes) => {}
            // TODO: better error handling
            _ => unimplemented!(),
        }
    }
}
