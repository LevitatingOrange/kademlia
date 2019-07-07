use crate::bucket::Bucket;
use crate::codec::PeerCodec;
use crate::storage::*;
use crate::util::*;
use actix::dev::ToEnvelope;
use actix::prelude::*;
use generic_array::GenericArray;
use log::{error, info, warn};
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::iter::FromIterator;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::{UdpFramed, UdpSocket};
use tokio::prelude::Stream;

pub trait MainActor<S: StorageActor> = 'static
    + Actor<Context: ToEnvelope<Self, ControllerResponse<S>>>
    + Handler<ControllerResponse<S>>;

#[derive(Debug, Message)]
pub enum ControllerMessage<S: StorageActor> {
    StoreKey(KademliaKey, S::StorageData),
    Retrieve(KademliaKey),
    Shutdown,
}
#[derive(Debug, Message)]
pub enum ControllerResponse<S: StorageActor> {
    Found(KademliaKey, S::StorageData),
    NotFound(KademliaKey),
    Shutdown
}

// pub enum GeneratorMessage {
//     Start,
//     Timeout,
//     Found,
// }

#[derive(Debug, Message)]
pub enum ControlMessage<S: StorageActor> {
    AddPeer(Connection),
    FindNode(Connection, KademliaKey, bool),
    FoundNodes(Connection, KademliaKey, Vec<Connection>),
    FoundKey(Connection, KademliaKey, S::StorageData),
    ReturnNodes(Connection, KademliaKey, Vec<Connection>),
    StoreKey(Connection, KademliaKey, S::StorageData),
    //HasKey(Connection, KademliaKey, bool),
    Shutdown,
}

#[derive(Debug, Message)]
pub enum InformationMessage {
    ChangedBucket(usize, Vec<Connection>),
}

pub fn create_controller<K, C, S, M>(
    id: KademliaKey,
    first_peer_id: KademliaKey,
    base_addr: SocketAddr,
    first_peer_addr: SocketAddr,
    bucket_size: usize,
    ping_timeout: Duration,
    find_timeout: Duration,
    storage_address: Addr<S>,
    main_address: Option<Addr<M>>,
) -> Addr<Controller<K, C, S, M>>
where
    K: FindNodeCount,
    C: ConcurrenceCount,
    S: StorageActor,
    M: MainActor<S>, //D: Handler<KademliaKeyMessage, Result=KademliaKey>,
{
    Controller::create(move |_| {
        Controller::new(
            id,
            base_addr,
            Connection::new(first_peer_addr, first_peer_id),
            bucket_size,
            ping_timeout,
            find_timeout,
            storage_address,
            main_address,
        )
    })
}

pub struct Controller<K: FindNodeCount, C: ConcurrenceCount, S: StorageActor, M: MainActor<S>> {
    id: KademliaKey,
    bucket_addresses: Option<GenericArray<Addr<Bucket<K, C, S, M>>, BucketListSize>>,
    buckets: GenericArray<Vec<Connection>, BucketListSize>,
    base_addr: SocketAddr,
    first_peer: Connection,
    bucket_size: usize,
    ping_timeout: Duration,
    find_timeout: Duration,
    find_timeout_handles: HashMap<KademliaKey, SpawnHandle>,
    find_list: Vec<OrderedConnection>,
    storage_address: Addr<S>,
    main_address: Option<Addr<M>>,
    k_nearest_callback: Option<Box<dyn FnOnce(&mut Self, &mut Context<Self>)>>,
    find_data: bool,
    find_active: bool,
}

impl<K, C, S, M> Controller<K, C, S, M>
where
    K: FindNodeCount,
    C: ConcurrenceCount,
    S: StorageActor,
    M: MainActor<S>,
{
    fn new(
        id: KademliaKey,
        base_addr: SocketAddr,
        mut first_peer: Connection,
        bucket_size: usize,
        ping_timeout: Duration,
        find_timeout: Duration,
        storage_address: Addr<S>,
        main_address: Option<Addr<M>>,
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
            storage_address,
            main_address,
            k_nearest_callback: None,
            find_data: false,
            find_active: false,
        }
    }

    fn to_ord(&self, conn: &Connection) -> OrderedConnection {
        OrderedConnection::new(conn.address, conn.id, self.id.distance(&conn.id))
    }

    fn find_k_nearest_locally(&self, id: KademliaKey) -> Vec<OrderedConnection> {
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
        id: KademliaKey,
        neighbour: OrderedConnection,
    ) {
        let conv_neighbour = Connection::from(neighbour);
        self.send_message(
            conv_neighbour,
            ControlMessage::FindNode(conv_neighbour, id, self.find_data),
        );
        self.find_timeout_handles.insert(
            neighbour.id,
            ctx.run_later(self.find_timeout, move |act: &mut Self, mut inner_ctx| {
                //println!("TEST2");
                if let Ok(i) = act.find_list.binary_search(&neighbour) {
                    act.find_list.remove(i);
                }
                act.find_timeout_handles.remove(&neighbour.id);
                act.send_find_to_next(&mut inner_ctx, id);
            }),
        );
    }

    fn send_message(&self, conn: Connection, message: ControlMessage<S>) {
        let index = self.id.bucket_index(&conn.id);
        if let Err(err) = self.bucket_addresses.as_ref().unwrap()[index].try_send(message) {
            error!("Bucket {} responded with error: {}", index, err);
        }
    }

    fn send_find_to_next(&mut self, ctx: &mut Context<Self>, id: KademliaKey) {
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
                if let Some(f) = self.k_nearest_callback.take() {
                    f(self, ctx);
                }
                self.k_nearest_callback = None;
                self.find_active = false;
                self.find_list.truncate(0);
                self.find_timeout_handles.clear();
                if self.find_data {
                    if let Some(addr) = &self.main_address {
                        if let Err(err) = addr.try_send(ControllerResponse::NotFound(id)) {
                            error!("Error talking with main actor, shutting down: {}", err);
                            ctx.stop();
                        }
                    } else {
                        warn!(
                            "No main actor connected, NotFound message for key {} is lost",
                            id
                        );
                    }

                //println!("No data found for key \"{}\"", id);
                // TODO send no key found here
                } else {
                    info!("Search done: {:?}", self.find_list);
                }
            }
        }
    }

    fn find_nodes(&mut self, id: KademliaKey) {
        self.find_data = false;
        self.find_k_nearest_globally(id);
    }

    fn find_data(&mut self, id: KademliaKey) {
        self.find_data = true;
        self.find_k_nearest_globally(id);
    }

    fn find_k_nearest_globally(&mut self, id: KademliaKey) {
        self.find_active = true;
        let nodes = self.find_k_nearest_locally(id);
        self.find_list = nodes;
        for d in self.find_list.iter().take(C::to_usize()) {
            let conn = Connection::from(*d);
            self.send_message(conn, ControlMessage::FindNode(conn, id, self.find_data));
        }
    }

    fn reorder_find_list(
        &mut self,
        ctx: &mut Context<Self>,
        conn: Connection,
        nodes: Vec<Connection>,
    ) {
        if let Some(handle) = self.find_timeout_handles.remove(&conn.id) {
            info!("Cancelling handle for conn {:?}", conn);
            ctx.cancel_future(handle);
        }
        if let Ok(i) = self.find_list.binary_search(&self.to_ord(&conn)) {
            self.find_list[i].visited = true;
            self.find_list[i].visiting = false;
        } 
        // else {
        //     error!("Could not find sending node in find list. This is a bug!");
        // }

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
                new_node.address.set_port(new_port);
                self.find_list.insert(i, new_node);
            }
        }
        // remove nodes that are too far away
        self.find_list.truncate(K::to_usize());
    }
}

impl<K, C, S, M> Actor for Controller<K, C, S, M>
where
    K: FindNodeCount,
    C: ConcurrenceCount,
    S: StorageActor,
    M: MainActor<S>,
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
                    let storage_addr = self.storage_address.clone();
                    Bucket::create(move |inner_ctx| {
                        let (w, r) = UdpFramed::new(socket, PeerCodec::default()).split();
                        Bucket::add_stream(r, inner_ctx);
                        Bucket::new(i, id, addr, bucket_size, ping_timeout, w, storage_addr)
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
            act.find_nodes(act.id);
        });
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        if let Some(d) = self.bucket_addresses.as_ref() {
            for addr in d {
                addr.do_send(ControlMessage::Shutdown);
            }
        }
        if let Some(m) = &self.main_address {
            // Main is still running, let it do the shutdown
            m.do_send(ControllerResponse::Shutdown);
        } else {
            // We are the main actor here, shut the system down
            System::current().stop();
        }
        Running::Stop
    }
}

impl<K: FindNodeCount, C: ConcurrenceCount, S: StorageActor, M: MainActor<S>>
    Handler<ControlMessage<S>> for Controller<K, C, S, M>
{
    type Result = ();

    fn handle(&mut self, msg: ControlMessage<S>, ctx: &mut Self::Context) {
        info!("Controller got message: {:?}", msg);
        match msg {
            ControlMessage::FindNode(peer, other_id, _) => {
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
                if !self.find_active {
                    return;
                }
                self.reorder_find_list(ctx, conn, nodes);
                self.send_find_to_next(ctx, id);

                // TODO: here we have to recompute the id's (do it before the
                // end) TODO: Does it make send that we do "got_contact" in
                // /find_node/found_node in bucket? It is not necessarily the
                // node we want in the buckets? Yes? because we expand our system that way
            }
            ControlMessage::FoundKey(conn, id, data) => {
                if !self.find_active {
                    return;
                }
                self.find_timeout_handles.clear();
                self.find_active = false;
                self.find_list.truncate(0);
                info!(
                    "Got data with id \"{}\" from peer (\"{}\"): \"{:?}\"",
                    id, conn, data
                );

                if let Some(addr) = &self.main_address {
                    if let Err(err) = addr.try_send(ControllerResponse::Found(id, data)) {
                        error!("Error talking with main actor, shutting down: {}", err);
                        ctx.stop();
                    }
                } else {
                    warn!("No main actor connected, found message for key {} with returned value {:?} is lost", id, data);
                }
                //self.reorder_find_list(ctx, conn, nodes);
                //self.send_find_to_next(ctx, id);

                // TODO: here we have to recompute the id's (do it before the
                // end) TODO: Does it make send that we do "got_contact" in
                // /find_node/found_node in bucket? It is not necessarily the
                // node we want in the buckets? Yes? because we expand our system that way
            }
            _ => error!("Controller got unrecognized message {:?}", msg),
        }
    }
}

impl<K: FindNodeCount, C: ConcurrenceCount, S: StorageActor, M: MainActor<S>>
    Handler<InformationMessage> for Controller<K, C, S, M>
{
    type Result = ();

    fn handle(&mut self, msg: InformationMessage, _: &mut Self::Context) {
        info!("Controller got message: {:?}", msg);
        match msg {
            InformationMessage::ChangedBucket(i, bucket) => {
                self.buckets[i] = bucket;
            }
        }
    }
}

impl<K: FindNodeCount, C: ConcurrenceCount, S: StorageActor, M: MainActor<S>>
    Handler<ControllerMessage<S>> for Controller<K, C, S, M>
{
    type Result = ();
    fn handle(&mut self, msg: ControllerMessage<S>, ctx: &mut Self::Context) {
        info!("Controller got controller message: {:?}", msg);
        match msg {
            ControllerMessage::StoreKey(id, data) => {
                self.k_nearest_callback = Some(Box::new(move |act, _| {
                    info!("Sending Store Request for key \"{}\" to {:?} found peers...", id, act.find_list.len());
                    for conn in &act.find_list {
                        let c = Connection::from(conn.clone());
                        act.send_message(c, ControlMessage::StoreKey(c, id, data.clone()));
                    }
                }));
                self.find_nodes(id);
            },
            ControllerMessage::Retrieve(id) => {
                self.find_data(id);
            }, 
            ControllerMessage::Shutdown => {
                ctx.stop();
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
