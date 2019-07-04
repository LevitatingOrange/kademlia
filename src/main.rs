use actix::prelude::System;
use generic_array::typenum::{U20, U3};
use kademlia::{create_controller, ControllerMessage, Key};
use std::env::args;
use std::net::SocketAddr;
use std::thread::sleep;
use std::time::Duration;

fn main() {
    let mut args = args();
    args.next().unwrap();
    let port: u16 = args.next().unwrap().parse().unwrap();
    let key: u128 = args.next().unwrap().parse().unwrap();
    let other_port: u16 = args.next().unwrap().parse().unwrap();
    let other_key: u128 = args.next().unwrap().parse().unwrap();
    env_logger::init();
    System::run(move || {
        let a = create_controller::<U20, U3>(
            Key(key),
            Key(other_key),
            SocketAddr::new("127.0.0.1".parse().unwrap(), port),
            SocketAddr::new("127.0.0.1".parse().unwrap(), other_port),
            20,
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        // let b = create_controller::<U20, U3>(
        //     Key(9824352),
        //     Key(2395872357),
        //     SocketAddr::new("127.0.0.1".parse().unwrap(), 9600),
        //     SocketAddr::new("127.0.0.1".parse().unwrap(), 9400),
        //     20,
        //     Duration::from_secs(60),
        //     Duration::from_secs(60),
        // );
        // let c = create_controller::<U20, U3>(
        //     Key(284219384),
        //     Key(9824352),
        //     SocketAddr::new("127.0.0.1".parse().unwrap(), 9800),
        //     SocketAddr::new("127.0.0.1".parse().unwrap(), 9600),
        //     20,
        //     Duration::from_secs(60),
        //     Duration::from_secs(60),
        // );
        //sleep(Duration::from_secs(10));
        //a.do_send(ControllerMessage::FindKNearest);
    })
    .unwrap();
    println!("TEST");
}
