use crate::bucket::Bucket;
use crate::codec::PeerCodec;
use crate::util::*;
use actix::prelude::*;
use generic_array::GenericArray;
use log::error;
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::iter::FromIterator;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::{UdpFramed, UdpSocket};
use tokio::prelude::Stream;

#[derive(Debug, Message)]
pub enum ControllerMessage {
    FindKNearest(Key),
}

// pub enum GeneratorMessage {
//     Start,
//     Timeout,
//     Found,
// }

#[derive(Debug, Message)]
pub enum ControlMessage {
    AddPeer(Connection),
    FindNode(Connection, Key),
    FoundNodes(Connection, Key, Vec<Connection>),
    ReturnNodes(Connection, Key, Vec<Connection>),
    Shutdown,
}

#[derive(Debug, Message)]
pub enum InformationMessage {
    ChangedBucket(usize, Vec<Connection>),
}

pub fn create_controller<K, C>(
    id: Key,
    first_peer_id: Key,
    base_addr: SocketAddr,
    first_peer_addr: SocketAddr,
    bucket_size: usize,
    ping_timeout: Duration,
    find_timeout: Duration,
) -> Addr<Controller<K, C>>
where
    K: FindNodeCount,
    C: ConcurrenceCount,
{
    Controller::create(move |_| {
        Controller::new(
            id,
            base_addr,
            Connection::new(first_peer_addr, first_peer_id),
            bucket_size,
            ping_timeout,
            find_timeout,
        )
    })
}

pub struct Controller<K: FindNodeCount, C: ConcurrenceCount> {
    id: Key,
    bucket_addresses: Option<GenericArray<Addr<Bucket<K, C>>, BucketListSize>>,
    buckets: GenericArray<Vec<Connection>, BucketListSize>,
    base_addr: SocketAddr,
    first_peer: Connection,
    bucket_size: usize,
    ping_timeout: Duration,
    find_timeout: Duration,
    find_timeout_handles: HashMap<Key, SpawnHandle>,
    find_list: Vec<OrderedConnection>,
}

impl<K, C> Controller<K, C>
where
    K: FindNodeCount,
    C: ConcurrenceCount,
{
    fn new(
        id: Key,
        base_addr: SocketAddr,
        mut first_peer: Connection,
        bucket_size: usize,
        ping_timeout: Duration,
        find_timeout: Duration,
    ) -> Self {
        let new_port = first_peer.address.port() + id.bucket_index(&first_peer.id) as u16;
        first_peer.address.set_port(new_port);

        Controller {
            id,
            bucket_addresses: None,
            // this gets updated by buckets
            buckets: GenericArray::default(),
            base_addr,
            first_peer,
            bucket_size,
            ping_timeout,
            find_timeout,
            find_timeout_handles: HashMap::new(),
            find_list: Vec::with_capacity(K::to_usize()),
        }
    }

    fn to_ord(&self, conn: &Connection) -> OrderedConnection {
        OrderedConnection::new(conn.address, conn.id, self.id.distance(&conn.id))
    }

    fn find_k_nearest_locally(&self, id: Key) -> Vec<OrderedConnection> {
        let conn_map = move |c: &Connection| self.to_ord(c);

        let original_bucket = self.id.bucket_index(&id);
        let mut prio_queue = BinaryHeap::from_iter(
            self.buckets[original_bucket]
                .iter()
                .filter(|c| c.id != id)
                .map(conn_map),
        );

        let mut around = 1;
        while prio_queue.len() < K::to_usize()
            && (original_bucket + around < BucketListSize::to_usize()
                || (original_bucket as i64) - (around as i64) >= 0)
        {
            if (original_bucket as i64) - (around as i64) >= 0 {
                for c in self.buckets[original_bucket - around].iter() {
                    prio_queue.push(conn_map(c));
                }
            }
            if original_bucket + around < BucketListSize::to_usize() {
                for c in self.buckets[original_bucket + around].iter() {
                    prio_queue.push(conn_map(c));
                }
            }
            around += 1;
        }
        let mut v: Vec<OrderedConnection> = prio_queue.into_sorted_vec();
        v.truncate(K::to_usize());
        v
    }

    fn find_k_nearest_of_neighbour(
        &mut self,
        ctx: &mut Context<Self>,
        id: Key,
        neighbour: OrderedConnection,
    ) {
        let conv_neighbour = Connection::from(neighbour);
        self.send_message(conv_neighbour, ControlMessage::FindNode(conv_neighbour, id));
        self.find_timeout_handles.insert(
            neighbour.id,
            ctx.run_later(self.find_timeout, move |act: &mut Self, mut inner_ctx| {
                if let Ok(i) = act.find_list.binary_search(&neighbour) {
                    act.find_list.remove(i);
                }
                act.send_find_to_next(&mut inner_ctx, id);
                // TODO: SEND new
                act.find_timeout_handles.remove(&neighbour.id);
            }),
        );
    }

    fn send_message(&self, conn: Connection, message: ControlMessage) {
        let index = self.id.bucket_index(&conn.id);
        if let Err(err) = self.bucket_addresses.as_ref().unwrap()[index].try_send(message) {
            error!("Bucket {} responded with error: {}", index, err);
        }
    }

    fn send_find_to_next(&mut self, ctx: &mut Context<Self>, id: Key) {
        // find next best element in our sorted list
        if let Some((i, _)) = self
            .find_list
            .iter()
            .enumerate()
            .skip_while(|(_, c)| c.visited || c.visiting)
            .next()
        {
            self.find_list[i].visiting = true;
            self.find_k_nearest_of_neighbour(ctx, id, self.find_list[i]);
        } else {
            if self.find_list.iter().all(|c| !c.visiting) {
                println!("Search done: {:?}", self.find_list);
                self.find_list.truncate(0);
                // we are done!
            }
        }
    }

    fn find_k_nearest_globally(&mut self, id: Key) {
        let nodes = self.find_k_nearest_locally(id);
        self.find_list = nodes;
        for d in self.find_list.iter().take(C::to_usize()) {
            let conn = Connection::from(*d);
            self.send_message(conn, ControlMessage::FindNode(conn, id));
        }
    }
}

impl<K, C> Actor for Controller<K, C>
where
    K: FindNodeCount,
    C: ConcurrenceCount,
{
    type Context = actix::Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let bucket_addresses = (0..BucketListSize::to_usize()).map(|i| {
            let mut addr = self.base_addr.clone();
            addr.set_port(addr.port() + (i as u16));
            match UdpSocket::bind(&addr) {
                Ok(socket) => {
                    let addr = ctx.address();
                    let id = self.id.clone();
                    let bucket_size = self.bucket_size.clone();
                    let ping_timeout = self.ping_timeout.clone();
                    Bucket::create(move |inner_ctx| {
                        let (w, r) = UdpFramed::new(socket, PeerCodec::default()).split();
                        Bucket::add_stream(r, inner_ctx);
                        Bucket::new(i, id, addr, bucket_size, ping_timeout, w)
                    })
                }
                Err(err) => {
                    // TODO: make this propper
                    panic!("Could not create socket: {}", err);
                }
            }
        });
        self.bucket_addresses = Some(GenericArray::from_exact_iter(bucket_addresses).unwrap());
        self.send_message(self.first_peer, ControlMessage::AddPeer(self.first_peer));

        // do it later so buckets get the first peer
        ctx.run_later(Duration::from_millis(500), move |act: &mut Self, _| {
            act.find_k_nearest_globally(act.id);
        });
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        if let Some(d) = self.bucket_addresses.as_ref() {
            for addr in d {
                addr.do_send(ControlMessage::Shutdown);
            }
        }
        Running::Stop
    }
}

impl<K: FindNodeCount, C: ConcurrenceCount> Handler<ControlMessage> for Controller<K, C> {
    type Result = ();

    fn handle(&mut self, msg: ControlMessage, ctx: &mut Self::Context) {
        println!("Controller got message: {:?}", msg);
        match msg {
            ControlMessage::FindNode(peer, other_id) => {
                // TODO handle too few nodes in bucket
                // // Send message with request for their nodes
                // // recompute port by using -d and adding new bucket_index
                // if let Err(err) = self.bucket_addresses.as_ref().unwrap()[d].try_send(ControlMessage::FindNode(peer, other_id))
                // {
                //     error!("Bucket {} responded with error: {}", d, err);
                // }
                self.send_message(
                    peer,
                    ControlMessage::ReturnNodes(
                        peer,
                        other_id,
                        self.find_k_nearest_locally(other_id)
                            .into_iter()
                            .map(Connection::from)
                            .collect(),
                    ),
                );
            }
            ControlMessage::FoundNodes(conn, id, nodes) => {
                if let Some(handle) = self.find_timeout_handles.remove(&conn.id) {
                    ctx.cancel_future(handle);
                }
                if let Ok(i) = self.find_list.binary_search(&self.to_ord(&conn)) {
                    self.find_list[i].visited = true;
                    self.find_list[i].visiting = false;
                } else {
                    error!("Could not find sending node in find list. This is a bug!");
                }

                // put new found nodes in our list and correct their ports (each port corresponds to a bucket)
                for new_node in nodes {
                    let mut new_node = self.to_ord(&new_node);
                    // insert in correct place
                    if let Err(i) = self.find_list.binary_search(&new_node) {
                        // Crucial bit: we recompute the port by subtracting the
                        // bucket index in the sending node to get the baseport
                        // of `new_node` and adding the bucket index here. This
                        // is because each bucket only inserts nodes to it's own
                        // socket because this makes bookkepping way easier.

                        // TODO: not sure if correct here. The used bucket seems
                        // wrong? Investigate
                        let new_port = new_node.address.port()
                            - (conn.id.bucket_index(&new_node.id) as u16)
                            + (self.id.bucket_index(&new_node.id) as u16);
                        println!(
                            "Prev: {}, Base: {}, New: {}",
                            new_node.address.port(),
                            new_node.address.port() - (conn.id.bucket_index(&new_node.id) as u16),
                            new_port
                        );
                        new_node.address.set_port(new_port);
                        self.find_list.insert(i, new_node);
                    }
                }
                // remove nodes that are too far away
                self.find_list.truncate(K::to_usize());

                self.send_find_to_next(ctx, id);

                // TODO: here we have to recompute the id's (do it before the
                // end) TODO: Does it make send that we do "got_contact" in
                // /find_node/found_node in bucket? It is not necessarily the
                // node we want in the buckets? Yes? because we expand our system that way
            }
            _ => error!("Controller got unrecognized message {:?}", msg),
        }
    }
}

impl<K: FindNodeCount, C: ConcurrenceCount> Handler<InformationMessage> for Controller<K, C> {
    type Result = ();

    fn handle(&mut self, msg: InformationMessage, _: &mut Self::Context) {
        println!("Controller got message: {:?}", msg);
        match msg {
            InformationMessage::ChangedBucket(i, bucket) => {
                self.buckets[i] = bucket;
            }
        }
    }
}

impl<K: FindNodeCount, C: ConcurrenceCount> Handler<ControllerMessage> for Controller<K, C> {
    type Result = ();
    fn handle(&mut self, msg: ControllerMessage, _: &mut Self::Context) {
        println!("Controller got controller message: {:?}", msg);
        match msg {
            ControllerMessage::FindKNearest(id) => {
                self.find_k_nearest_globally(id);
            }
        }
    }
}
// impl<K: FindNodeCount> ResultHandler<InformationMessage> for Controller<K> {

//     fn handle(&mut self, msg: ControlMessage, ctx: &mut Self::Context) {
//         match msg {
//             ControlMessage::FindNode(peer, other_id) => {
//                 // TODO handle too few nodes in bucket
//                 // let d = self.id.bucket_index(&other_id);
//                 // // Send message with request for their nodes
//                 // // recompute port by using -d and adding new bucket_index
//                 // if let Err(err) = self.buckets.as_ref().unwrap()[d].try_send(ControlMessage::FindNode(peer, other_id))
//                 // {
//                 //     error!("Bucket {} responded with error: {}", d, err);
//                 // }

//             }
//             _ => error!("Controller got unrecognized message {:?}", msg),
//         }
//     }
// }
