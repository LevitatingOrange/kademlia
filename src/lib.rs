#![feature(trait_alias)]
#![feature(associated_type_bounds)]
#![feature(core_intrinsics)]

mod bucket;
mod codec;
mod controller;
mod storage;
mod util;

pub use controller::{create_controller, Controller, ControllerMessage};
pub use storage::*;
pub use util::KademliaKey;

// #[cfg(test)]
// mod tests {
//     #[test]
//     fn it_works() {
//         assert_eq!(2 + 2, 4);
//     }
// }
