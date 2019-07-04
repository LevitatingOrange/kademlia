use crate::codec::{PeerCodec, PeerMessage};
use crate::controller::Controller;
use crate::controller::{ControlMessage, InformationMessage};
use crate::util::*;
use actix::prelude::*;
use log::{error, info};
use std::collections::HashMap;
use std::io;
use std::mem::replace;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpFramed;
use tokio::prelude::stream::SplitSink;
use tokio::prelude::Sink;

pub struct Bucket<K: FindNodeCount, C: ConcurrenceCount> {
    own_index: usize,
    own_id: Key,
    controller: Addr<Controller<K, C>>,
    //bucket_index: usize,
    bucket_size: usize,
    bucket: Vec<Connection>,
    queue: Vec<Connection>,
    ping_timeout: Duration,
    ping_timeout_handles: HashMap<Key, SpawnHandle>,
    // write part of the udp stream wrapped so that Messages are deserialized automatically
    framed_write_stream: SplitSink<UdpFramed<PeerCodec<K>>>,
}

impl<K: FindNodeCount, C: ConcurrenceCount> Bucket<K, C> {
    pub fn new(
        own_index: usize,
        own_id: Key,
        controller: Addr<Controller<K, C>>,
        bucket_size: usize,
        ping_timeout: Duration,
        framed_write_stream: SplitSink<UdpFramed<PeerCodec<K>>>,
    ) -> Self {
        Bucket {
            own_index,
            own_id,
            controller,
            bucket_size,
            bucket: Vec::with_capacity(bucket_size),
            queue: Vec::new(),
            ping_timeout,
            ping_timeout_handles: HashMap::new(),
            framed_write_stream,
        }
    }

    fn got_contact(&mut self, ctx: &mut Context<Bucket<K, C>>, peer: Connection) {
        if let Some((i, _)) = self.bucket.iter().enumerate().find(|(_, c)| &peer == *c) {
            let d = self.bucket.remove(i);
            self.bucket.push(d);
        } else if self.bucket.len() < self.bucket_size {
            self.bucket.push(peer);
        } else {
            self.queue.push(peer);
            // TODO: what happens if two contacts happen in-between the pin
            let old_best_peer = self.bucket.last().unwrap().clone();
            if let Err(err) = (&mut self.framed_write_stream)
                .send((PeerMessage::Ping, old_best_peer.address))
                .wait()
            {
                error!("Sink error: {}, stopping actor!", err);
                ctx.stop();
            } else {
                self.ping_timeout_handles.insert(old_best_peer.id, ctx.run_later(self.ping_timeout, move |act: &mut Bucket<K, C>, &mut _| {
                    if let Some(index) = act.bucket.iter().position(|p| p.id == old_best_peer.id) {
                        // TODO remove and insert same place
                        if let Some(new_peer) = act.queue.pop() {
                            let old = replace(&mut act.bucket[index], new_peer);
                            info!("Ping timed out to {}, using cached peer", old);
                        } else {
                            let old = act.bucket.remove(index);
                            info!("Ping timed out to {}, cannot use peer from cache as the cache empty!", old);
                        }
                    } else {
                        if let Some(new_peer) = act.queue.pop() {
                        info!("Peer not found in bucket, moving cached peer to top");
                            act.bucket.push(new_peer);
                        } else {
                            info!("Bucket was empty, Cache was empty");
                        }
                    }
                    act.ping_timeout_handles.remove(&old_best_peer.id);
                }));
            }
        }
        self.controller.do_send(InformationMessage::ChangedBucket(
            self.own_index,
            self.bucket.to_vec(),
        ));
    }

    fn send_with_err_handling(
        &mut self,
        peer: Connection,
        msg: PeerMessage<K>,
        ctx: &mut Context<Self>,
    ) {
        if let Err(err) = (&mut self.framed_write_stream)
            .send((msg, peer.address))
            .wait()
        {
            error!("Sink error: {}, stopping actor!", err);
            ctx.stop();
        }
    }

    fn send_to_controller(&mut self, msg: ControlMessage, ctx: &mut Context<Self>) {
        if let Err(err) = self.controller.try_send(msg) {
            error!("Controller error: {}, stopping actor!", err);
            ctx.stop();
        }
    }
}

impl<K: FindNodeCount, C: ConcurrenceCount> Actor for Bucket<K, C> {
    type Context = actix::Context<Self>;

    fn started(&mut self, _: &mut Self::Context) {
        // TODO
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        info!("Stopping bucket actor!");
        // TODO send shutdown info to peers
        Running::Stop
    }
}

impl<K: FindNodeCount, C: ConcurrenceCount> Handler<ControlMessage> for Bucket<K, C> {
    type Result = ();

    fn handle(&mut self, msg: ControlMessage, ctx: &mut Self::Context) {
        println!(
            "Bucket {} got message control message {:?}",
            self.own_index, msg
        );
        match msg {
            ControlMessage::FindNode(conn, key) => {
                self.send_with_err_handling(
                    conn,
                    PeerMessage::FindNode {
                        id: key,
                        sender_id: self.own_id,
                    },
                    ctx,
                );
            }
            ControlMessage::AddPeer(peer) => {
                self.got_contact(ctx, peer);
            }
            ControlMessage::ReturnNodes(peer, id, nodes) => {
                //let mut nodes = self.bucket.to_vec();
                // nodes.truncate(K::to_usize());
                // for node in &mut nodes {
                //     let old_port = node.address.port();
                //     node.address.set_port(
                //         old_port - self.own_id.bucket_index(&node.id) as u16
                //             + id.bucket_index(&node.id) as u16,
                //     );
                // }
                self.send_with_err_handling(
                    peer,
                    PeerMessage::FoundNodes {
                        id,
                        sender_id: self.own_id,
                        nodes,
                    },
                    ctx,
                );
            }
            ControlMessage::Shutdown => {
                info!("Got shutdown message, stopping...!");
                ctx.stop();
            }
            _ => error!("Bucket: unrecognized message {:?}", msg),
        }
    }
}

impl<K, C> StreamHandler<(PeerMessage<K>, SocketAddr), io::Error> for Bucket<K, C>
where
    K: FindNodeCount,
    C: ConcurrenceCount,
{
    fn handle(&mut self, msg: (PeerMessage<K>, SocketAddr), ctx: &mut Self::Context) {
        println!("Bucket {} got peer message {:?}", self.own_index, msg);
        // Got pong from peer, so we don't replace it with the first peer in the cache.
        match msg {
            (PeerMessage::Pong(id), _) => {
                if let Some(handle) = self.ping_timeout_handles.remove(&id) {
                    ctx.cancel_future(handle);
                }
            }
            (PeerMessage::FindNode { sender_id, id }, sender_addr) => {
                let conn = Connection::new(sender_addr, sender_id);
                // let mut nodes = (&self.bucket).to_vec();
                // nodes.truncate(K::to_usize());
                // TODO: Blocking ok here? Error handling?
                // if let Err(err) = (&mut self.framed_write_stream)
                //     .send((PeerMessage::FoundNodes(nodes), sender_addr))
                //     .wait()
                self.send_to_controller(ControlMessage::FindNode(conn, id), ctx);
                self.got_contact(ctx, conn);
            }
            (
                PeerMessage::FoundNodes {
                    sender_id,
                    id,
                    nodes,
                },
                sender_addr,
            ) => {
                let sender = Connection::new(sender_addr, sender_id);
                self.send_to_controller(ControlMessage::FoundNodes(sender, id, nodes), ctx);
                self.got_contact(ctx, sender);
            }
            //PeerMessage::FoundNodes(nodes) => {}
            // TODO: better error handling
            _ => unimplemented!(),
        }
    }
}
