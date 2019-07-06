use actix::prelude::*;
use console::Term;
use generic_array::typenum::{U20, U3};
use log::{error, info};
use std::collections::HashMap;
use std::env::args;
use std::error::Error;
use std::net::SocketAddr;
use std::thread;
use std::time::Duration;

use dialoguer::{Input, Select};

use kademlia::*;

#[derive(Debug)]
pub struct SimpleStorageActor {
    storage: HashMap<KademliaKey, String>,
}

impl SimpleStorageActor {
    fn new() -> Self {
        SimpleStorageActor {
            storage: HashMap::new(),
        }
    }
}

impl StorageActor for SimpleStorageActor {
    type StorageData = String;
}

impl Actor for SimpleStorageActor {
    type Context = actix::Context<Self>;

    fn started(&mut self, _: &mut Self::Context) {}

    fn stopped(&mut self, _: &mut Self::Context) {}
}

impl Handler<StorageMessage<Self>> for SimpleStorageActor {
    type Result = StorageResult<Self>;
    fn handle(&mut self, msg: StorageMessage<Self>, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            StorageMessage::Store(id, data) => {
                info!("Storing \"{}\" with id \"{}\"", data, id);
                self.storage.insert(id, data);
                // TODO this is a bit ugly because if we store we do not need a result
                StorageResult::NotFound
            }
            StorageMessage::Retrieve(id) => {
                if let Some(data) = self.storage.get(&id) {
                    StorageResult::Found(data.clone())
                } else {
                    StorageResult::NotFound
                }
            } // msg => {
              //     error!("Unrecognized storage message: {:?}", msg);
              //     ctx.stop();
              //     StorageResult::NotFound
              // }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = args();
    args.next()
        .ok_or("Usage:  kad-bin <our_port> <our_key> <first_peer_port> <first_peer_key>")?;
    let port: u16 = args
        .next()
        .ok_or("Usage:  kad-bin <our_port> <our_key> <first_peer_port> <first_peer_key>")?
        .parse()?;
    let key: u32 = args
        .next()
        .ok_or("Usage:  kad-bin <our_port> <our_key> <first_peer_port> <first_peer_key>")?
        .parse()?;
    let other_port: u16 = args
        .next()
        .ok_or("Usage:  kad-bin <our_port> <our_key> <first_peer_port> <first_peer_key>")?
        .parse()?;
    let other_key: u32 = args
        .next()
        .ok_or("Usage:  kad-bin <our_port> <our_key> <first_peer_port> <first_peer_key>")?
        .parse()?;

    env_logger::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    System::run(move || {
        let storage_address = SimpleStorageActor::create(move |_| SimpleStorageActor::new());

        let controller_address = create_controller::<U20, U3, SimpleStorageActor>(
            KademliaKey(key),
            KademliaKey(other_key),
            SocketAddr::new("127.0.0.1".parse().unwrap(), port),
            SocketAddr::new("127.0.0.1".parse().unwrap(), other_port),
            20,
            Duration::from_secs(60),
            Duration::from_secs(60),
            storage_address,
        );
        thread::spawn(move || {
            let term = Term::stdout();
            term.clear_screen().unwrap();
            term.move_cursor_down(3).unwrap();
            let mut select = Select::new();
            select.item("Retrieve key");
            select.item("Store key");
            select.default(0);
            loop {
                term.move_cursor_up(3).unwrap();
                let which = select.interact().unwrap();
                let id_str = Input::<String>::new()
                    .with_prompt("Enter id (Ctr-C to quit)")
                    .interact()
                    .unwrap();

                if let Ok(id) = id_str.parse() {
                    let key = KademliaKey(id);
                    if which == 0 {
                        controller_address.do_send(ControllerMessage::Retrieve(key));
                    } else {
                        let data = Input::<String>::new()
                            .with_prompt(&format!(
                                "Enter data to store for key \"{}\" (Ctr-C to quit)",
                                key
                            ))
                            .interact()
                            .unwrap();

                        controller_address.do_send(ControllerMessage::StoreKey(key, data));
                    }
                } else {
                    println!("Could not read id, please enter number!");
                    continue;
                }
                term.move_cursor_down(3).unwrap();
            }
        });

        let ctrl_c = tokio_signal::ctrl_c().flatten_stream();
        let handle_shutdown = ctrl_c
            .for_each(|()| {
                println!("Ctrl-C received, shutting down");
                System::current().stop();
                Ok(())
            })
            .map_err(|_| ());
        actix::spawn(handle_shutdown);
    })?;

    Ok(())
    //println!("TEST");
}
